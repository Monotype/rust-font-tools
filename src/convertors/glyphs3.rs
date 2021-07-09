use crate::common::OTValue;
use crate::glyph::GlyphCategory;
use crate::i18ndictionary::I18NDictionary;
use crate::OTScalar::Signed;
use crate::Shape::{ComponentShape, PathShape};
use crate::{Anchor, OTScalar};
use crate::{
    Axis, BabelfontError, Component, Font, Glyph, Guide, Instance, Layer, Location, Master, Node,
    NodeType, Path, Position, Shape,
};
use openstep_plist::{Plist, PlistDictionary, PlistParser};
use std::collections::HashMap;
use std::convert::TryInto;

use chrono::TimeZone;
use lazy_static::lazy_static;
use std::fs;
use std::path::PathBuf;

pub fn load(path: PathBuf) -> Result<Font, BabelfontError> {
    let s = fs::read_to_string(&path).map_err(|source| BabelfontError::IO {
        path: path.clone(),
        source,
    })?;
    let rawplist = PlistParser::parse(s, true).map_err(|source| BabelfontError::PlistParse {
        path: path.clone(),
        source,
    })?;
    let plist = match rawplist {
        Plist::Dictionary(p) => p,
        _ => {
            return Err(BabelfontError::General {
                msg: "Top level of plist wasn't a dictionary".to_string(),
            })
        }
    };
    if !plist.contains_key(".formatVersion") {
        return Err(BabelfontError::WrongConvertor { path });
    }

    let mut font = Font::new();

    let custom_parameters = get_custom_parameters(&plist);
    load_axes(&mut font, &plist);
    // load_kern_groups(&mut font, &plist);
    load_masters(&mut font, &plist)?;
    let default_master_id = custom_parameters
        .get(&"Variable Font Origin")
        .and_then(|x| x.string())
        .cloned()
        .or_else(|| font.masters.first().map(|m| m.id.clone()));

    fixup_axes(&mut font, default_master_id.as_ref());
    load_glyphs(&mut font, &plist);

    if let Some(instances) = plist.get("instances").map(|f| f.iter_array_of_dicts()) {
        for instance in instances {
            load_instance(&mut font, &instance);
        }
    }

    fixup_axis_mappings(&mut font);
    load_metadata(&mut font, &plist);

    load_custom_parameters(&mut font.custom_ot_values, custom_parameters);

    // load_features(&mut font, &plist);
    Ok(font)
}

fn get_custom_parameters(plist: &PlistDictionary) -> HashMap<&str, &Plist> {
    let mut cp: HashMap<&str, &Plist> = HashMap::new();
    if let Some(param) = plist.get("customParameters") {
        for p in param.iter_array_of_dicts() {
            let key = p.get("name").and_then(|n| n.string());
            let value = p.get("value");
            if let Some(key) = key {
                if let Some(value) = value {
                    cp.insert(key, value);
                }
            }
        }
    }

    cp
}

fn load_axes(font: &mut Font, plist: &PlistDictionary) {
    if let Some(axes) = plist.get("axes") {
        for axis in axes.iter_array_of_dicts() {
            let name = axis.get("name").and_then(|n| n.string());
            let tag = axis.get("tag").and_then(|n| n.string());
            if let Some(name) = name {
                if let Some(tag) = tag {
                    let mut new_axis = Axis::new(name, tag.to_string());
                    new_axis.hidden = axis.contains_key("hidden");
                    font.axes.push(new_axis)
                }
            }
        }
    }
}

fn _to_loc(font: &Font, values: Option<&Plist>) -> Location {
    let axis_tags = font.axes.iter().map(|x| x.tag.clone());
    let mut loc = Location::new();
    if let Some(values) = values.and_then(|v| v.array()) {
        for (v, tag) in values.iter().zip(axis_tags) {
            loc.0.insert(tag, v.into());
        }
    }
    loc
}

fn convert_metric_name(n: &str) -> String {
    (match n {
        "x-height" => "xHeight",
        "cap height" => "capHeight",
        _ => n,
    })
    .to_string()
}
fn load_masters(font: &mut Font, plist: &PlistDictionary) -> Result<(), BabelfontError> {
    let metrics = plist.get("metrics");
    if let Some(masters) = plist.get("fontMaster") {
        for master in masters.iter_array_of_dicts() {
            let location = _to_loc(font, master.get("axesValues"));
            let name =
                master
                    .get("name")
                    .and_then(|n| n.string())
                    .ok_or(BabelfontError::General {
                        msg: "Master has no name!".to_string(),
                    })?;
            let id = master
                .get("id")
                .and_then(|n| n.string())
                .ok_or(BabelfontError::General {
                    msg: "Master has no id!".to_string(),
                })?;
            let mut new_master = Master::new(name, id, location);

            if let Some(guides) = master.get("guides").and_then(|a| a.array()) {
                new_master.guides = guides.iter().map(|g| load_guide(g)).collect();
            }

            load_metrics(&mut new_master, master, metrics);
            if let Some(kerning) = master.get("kerningLTR").and_then(|a| a.dict()) {
                // load_kerning(new_master, kerning);
            }
            let custom_parameters = get_custom_parameters(master);
            load_custom_parameters(&mut new_master.custom_ot_values, custom_parameters);
            font.masters.push(new_master)
        }
    }
    Ok(())
}

fn load_metrics(new_master: &mut Master, master: &PlistDictionary, metrics: Option<&Plist>) {
    if let Some(metric_values) = master.get("metricValues").and_then(|l| l.array()) {
        if let Some(metrics) = metrics {
            for (metric, metric_value) in metrics.iter_array_of_dicts().zip(metric_values.iter()) {
                if let Some(metric_name) = metric
                    .get("type")
                    .or_else(|| metric.get("name"))
                    .and_then(|x| x.string())
                {
                    let value: i32 = metric_value
                        .dict()
                        .and_then(|d| d.get("pos"))
                        .unwrap_or(&Plist::Integer(0))
                        .into();
                    new_master
                        .metrics
                        .insert(convert_metric_name(metric_name), value);
                    // I don't care about overshoots today.
                }
            }
        }
    }
}

fn tuple_to_position(p: &[Plist]) -> Position {
    let mut x: f32 = 0.0;
    let mut y: f32 = 0.0;
    let mut angle: f32 = 0.0;
    let mut iter = p.iter();
    if let Some(x_plist) = iter.next() {
        x = x_plist.into();
    }
    if let Some(y_plist) = iter.next() {
        y = y_plist.into();
    }
    if let Some(angle_plist) = iter.next() {
        angle = angle_plist.into();
    }

    Position {
        x: x as i32,
        y: y as i32,
        angle,
    }
}

fn load_guide(g: &Plist) -> Guide {
    let mut guide = Guide::new();
    let default = vec![Plist::Integer(0), Plist::Integer(0)];
    if let Some(g) = g.dict() {
        let pos = g.get("pos").and_then(|x| x.array()).unwrap_or(&default);
        let angle: f32 = g
            .get("angle")
            .unwrap_or(&Plist::Float(0.0))
            .try_into()
            .unwrap_or(0.0);
        guide.pos = tuple_to_position(pos);
        guide.pos.angle = angle;
    }
    guide
}

fn fixup_axes(f: &mut Font, default_master_id: Option<&String>) {
    for master in &f.masters {
        for mut axis in f.axes.iter_mut() {
            let this_loc = *(master.location.0.get(&axis.tag).unwrap_or(&0.0));
            if axis.min.is_none() || this_loc < axis.min.unwrap() {
                axis.min = Some(this_loc);
            }
            if axis.max.is_none() || this_loc > axis.max.unwrap() {
                axis.max = Some(this_loc);
            }
            if default_master_id == Some(&master.id) {
                axis.default = Some(this_loc);
            }
        }
    }
}

fn load_glyphs(font: &mut Font, plist: &PlistDictionary) {
    if let Some(glyphs) = plist.get("glyphs").and_then(|a| a.array()) {
        for g in glyphs {
            if let Some(g) = g.dict() {
                if let Ok(glyph) = load_glyph(g) {
                    font.glyphs.push(glyph);
                }
            }
        }
    }
}

fn load_glyph(g: &PlistDictionary) -> Result<Glyph, ()> {
    let name = g.get("glyphname").and_then(|f| f.string()).ok_or(())?;
    let category = g.get("category").and_then(|f| f.string());
    let subcategory = g.get("subcategory").and_then(|f| f.string());
    let codepoints = get_codepoints(g);
    let gc = if subcategory == Some(&"Ligature".to_string()) {
        GlyphCategory::Ligature
    } else if category == Some(&"Mark".to_string()) {
        GlyphCategory::Mark
    } else {
        GlyphCategory::Base
    };
    let mut layers = vec![];
    if let Some(plist_layers) = g.get("layers") {
        for layer in plist_layers.iter_array_of_dicts() {
            layers.push(load_layer(layer)?);
        }
    }
    Ok(Glyph {
        name: name.to_string(),
        category: gc,
        production_name: None,
        codepoints,
        layers,
        exported: !g.contains_key("export"),
        direction: None,
    })
}

fn load_layer(l: &PlistDictionary) -> Result<Layer, ()> {
    let width = l.get("width").map(i32::from).unwrap_or(0);
    let mut layer = Layer::new(width);
    if let Some(name) = l.get("width").and_then(|l| l.string()) {
        layer.name = Some(name.to_string());
    }
    if let Some(id) = l.get("layerId").and_then(|l| l.string()) {
        layer.id = Some(id.to_string());
    }
    if let Some(guides) = l.get("guides").and_then(|l| l.array()) {
        layer.guides = guides.iter().map(|x| load_guide(x)).collect();
    }
    if let Some(anchors) = l.get("anchors").map(|l| l.iter_array_of_dicts()) {
        for anchor in anchors {
            layer.anchors.push(load_anchor(anchor));
        }
    }
    if let Some(shapes) = l.get("shapes").map(|l| l.iter_array_of_dicts()) {
        for shape in shapes {
            layer.shapes.push(load_shape(shape)?);
        }
    }

    Ok(layer)
}

fn load_anchor(a: &PlistDictionary) -> Anchor {
    let default = vec![Plist::Integer(0), Plist::Integer(0)];
    let pos = a.get("pos").and_then(|x| x.array()).unwrap_or(&default);
    Anchor {
        x: pos.first().map(i32::from).unwrap_or(0),
        y: pos.last().map(i32::from).unwrap_or(0),
        name: a
            .get("name")
            .and_then(|x| x.string())
            .unwrap_or(&"Unknown".to_string())
            .to_string(),
    }
}

fn load_shape(a: &PlistDictionary) -> Result<Shape, ()> {
    if a.contains_key("nodes") {
        // It's a path
        let mut path = Path {
            nodes: vec![],
            closed: true,
            direction: crate::shape::PathDirection::Clockwise,
        };
        for node in a.get("nodes").unwrap().array().ok_or(())? {
            let node = node.array().ok_or(())?;
            let typ: Option<char> = node[2].string().map(|x| x.chars().next().unwrap_or('l'));
            let nodetype = match typ {
                Some('l') => NodeType::Line,
                Some('o') => NodeType::OffCurve,
                Some('c') => NodeType::Curve,
                _ => NodeType::Line,
            };
            path.nodes.push(Node {
                x: (&node[0]).into(),
                y: (&node[1]).into(),
                nodetype,
            })
        }
        Ok(PathShape(path))
    } else {
        // It's a component
        let reference = a.get("ref").and_then(|f| f.string()).ok_or(())?;
        let pos: Vec<f32> = a
            .get("pos")
            .and_then(|f| f.array())
            .unwrap_or(&vec![Plist::Integer(0), Plist::Integer(0)])
            .iter()
            .map(f32::from)
            .collect();

        let scale: Vec<f32> = a
            .get("scale")
            .and_then(|f| f.array())
            .unwrap_or(&vec![Plist::Integer(1), Plist::Integer(1)])
            .iter()
            .map(f32::from)
            .collect();
        let transform = kurbo::Affine::translate((
            *pos.first().unwrap_or(&0.0) as f64,
            *pos.last().unwrap_or(&0.0) as f64,
        ));
        let scalingtransform = kurbo::Affine::scale_non_uniform(
            *scale.first().unwrap_or(&1.0) as f64,
            *scale.last().unwrap_or(&1.0) as f64,
        );

        Ok(ComponentShape(Component {
            reference: reference.to_string(),
            transform: transform * scalingtransform,
        }))
    }
}

fn get_codepoints(g: &PlistDictionary) -> Vec<usize> {
    let unicode = g.get("unicode");
    if unicode.is_none() {
        return vec![];
    }
    let unicode = unicode.unwrap();
    if let Plist::Array(unicodes) = unicode {
        return unicodes.iter().map(|x| i32::from(x) as usize).collect();
    } else {
        return vec![i32::from(unicode) as usize];
    }
}

fn load_metadata(font: &mut Font, plist: &PlistDictionary) {
    font.upm = i32::from(plist.get("unitsPerEm").unwrap_or(&Plist::Integer(1000))) as u16;
    font.version = (
        i32::from(plist.get("versionMajor").unwrap_or(&Plist::Integer(1))) as u16,
        i32::from(plist.get("versionMinor").unwrap_or(&Plist::Integer(0))) as u16,
    );
    font.names.family_name = plist
        .get("familyName")
        .and_then(|s| s.string())
        .unwrap_or(&"New font".to_string())
        .into();
    load_properties(font, &plist);
    font.date = plist
        .get("date")
        .and_then(|x| x.string())
        .and_then(|x| chrono::NaiveDateTime::parse_from_str(x, "%Y-%m-%d %H:%M:%S +0000").ok())
        .map(|x| chrono::Local.from_local_datetime(&x).unwrap())
        .unwrap_or_else(chrono::Local::now);
    font.note = plist
        .get("note")
        .and_then(|x| x.string())
        .map(|x| x.to_string());
}

fn load_properties(font: &mut Font, plist: &PlistDictionary) {
    if let Some(props) = plist.get("properties").map(|d| d.iter_array_of_dicts()) {
        for prop in props {
            if let Some(key) = prop.get("key").and_then(|f| f.string()) {
                let mut val = I18NDictionary::new();
                if let Some(pval) = prop.get("value").and_then(|f| f.string()) {
                    val.set_default(pval.to_string());
                } else if let Some(pvals) = prop.get("values").map(|f| f.iter_array_of_dicts()) {
                    for entry in pvals {
                        if let Some(l) = entry.get("language").and_then(|f| f.string()) {
                            if let Some(v) = entry.get("value").and_then(|f| f.string()) {
                                if l.len() != 4 {
                                    continue;
                                };
                                let l = l.as_bytes()[0..4].try_into().unwrap();
                                val.0.insert(l, v.to_string());
                            }
                        }
                    }
                }
                if key == "copyright" || key == "copyrights" {
                    font.names.copyright = val;
                } else if key == "designer" || key == "designers" {
                    font.names.designer = val;
                } else if key == "designerURL" {
                    font.names.designer_url = val;
                } else if key == "manufacturer" || key == "manufacturers" {
                    font.names.manufacturer = val;
                } else if key == "manufacturerURL" {
                    font.names.manufacturer_url = val;
                } else if key == "license" || key == "licenses" {
                    font.names.license = val;
                } else if key == "licenseURL" {
                    font.names.license_url = val;
                } else if key == "trademark" || key == "trademarks" {
                    font.names.trademark = val;
                } else if key == "description" || key == "descriptions" {
                    font.names.description = val;
                } else if key == "sampleText" || key == "sampleTexts" {
                    font.names.sample_text = val;
                } else if key == "postscriptFullName" { // ??
                } else if key == "WWSFamilyName" {
                    font.names.w_w_s_family_name = val;
                } else if key == "versionString" {
                    font.names.version = val;
                }
            }
        }
    }
}

lazy_static! {
    static ref UNSIGNED_CP: Vec<(&'static str, &'static str, &'static str)> =
        vec![
        ("openTypeHeadLowestRecPPEM", "head", "lowestRecPPEM"),
        ("openTypeOS2StrikeoutPosition", "OS2", "yStrikeoutPosition"),
        ("openTypeOS2WidthClass", "OS2", "usWidthClass"),
        ("openTypeOS2WeightClass", "OS2", "usWeightClass"),
        ("widthClass", "OS2", "usWidthClass"),
        ("weightClass", "OS2", "usWeightClass"),
        ("openTypeOS2WinAscent", "OS2", "usWinAscent"),
        ("openTypeOS2WinDescent", "OS2", "usWinDescent"),
        ("winAscent", "OS2", "usWinAscent"),
        ("winDescent", "OS2", "usWinDescent"),


        ];
    static ref SIGNED_CP: Vec<(&'static str, &'static str, &'static str)> = vec![
        ("hheaAscender", "hhea", "ascent"),
        ("openTypeHheaAscender", "hhea", "ascent"),
        ("hheaDescender", "hhea", "descent"),
        ("openTypeHheaDescender", "hhea", "descent"),
        ("hheaLineGap", "hhea", "lineGap"),
        ("openTypeHheaLineGap", "hhea", "lineGap"),
        ("openTypeOS2FamilyClass", "OS2", "sFamilyClass"),
        ("openTypeOS2StrikeoutPosition", "OS2", "yStrikeoutPosition"),
        ("openTypeOS2StrikeoutSize", "OS2", "yStrikeoutSize"),
        ("strikeoutPosition", "OS2", "yStrikeoutPosition"),
        ("strikeoutSize", "OS2", "yStrikeoutSize"),
        ("openTypeOS2SubscriptXOffset","OS2", "ySubscriptXOffset"),
        ("openTypeOS2SubscriptXSize","OS2", "ySubscriptXSize"),
        ("openTypeOS2SubscriptYOffset","OS2", "ySubscriptYOffset"),
        ("openTypeOS2SubscriptYSize","OS2", "ySubscriptYSize"),
        ("openTypeOS2SuperscriptXOffset","OS2", "ySuperscriptXOffset"),
        ("openTypeOS2SuperscriptXSize","OS2", "ySuperscriptXSize"),
        ("openTypeOS2SuperscriptYOffset","OS2", "ySuperscriptYOffset"),
        ("openTypeOS2SuperscriptYSize","OS2", "ySuperscriptYSize"),
        ("subscriptXOffset","OS2", "ySubscriptXOffset"),
        ("subscriptXSize","OS2", "ySubscriptXSize"),
        ("subscriptYOffset","OS2", "ySubscriptYOffset"),
        ("subscriptYSize","OS2", "ySubscriptYSize"),
        ("superscriptXOffset","OS2", "ySuperscriptXOffset"),
        ("superscriptXSize","OS2", "ySuperscriptXSize"),
        ("superscriptYOffset","OS2", "ySuperscriptYOffset"),
        ("superscriptYSize","OS2", "ySuperscriptYSize"),
        ("openTypeOS2TypoAscender","OS2", "sTypoAscender"),
        ("openTypeOS2TypoDescender","OS2", "sTypoDescender"),
        ("openTypeOS2TypoLineGap","OS2", "sTypoLineGap"),
        ("typoAscender","OS2", "sTypoAscender"),
        ("typoDescender","OS2", "sTypoDescender"),
        ("typoLineGap","OS2", "sTypoLineGap"),
        ("underlinePosition", "post", "underlinePosition"),
        ("postscriptUnderlinePosition", "post", "underlinePosition"),
        ("underlineThickness", "post", "underlineThickness"),
        ("postscriptUnderlineThickness", "post", "underlineThickness"),
        ("openTypeHheaCaretSlopeRun", "hhea", "caretSlopeRun"),
        ("openTypeVheaCaretSlopeRun", "vhea", "caretSlopeRun"),
        ("openTypeVheaCaretSlopeRise", "vhea", "caretSlopeRise"),
        ("openTypeHheaCaretSlopeRise", "hhea", "caretSlopeRise"),
        ("openTypeHheaCaretOffset", "hhea", "caretOffset"),

    ];
    static ref STRING_CP: Vec<(&'static str, &'static str, &'static str)> = vec![
        ("preferredFamilyName", "name", "preferredFamilyName"),
        ("openTypeNamePreferredFamilyName", "name", "preferredFamilyName"),
        ("preferredSubfamilyName", "name", "preferredSubfamilyName"),
        ("openTypeHheaDescender", "hhea", "descent"),
        ("compatibleFullName", "name", "compatibleFullName"),
        ("openTypeNameCompatibleFullName", "name", "compatibleFullName"),
        ("vendorID", "OS2", "achVendID"),
        ("openTypeOS2VendorID", "OS2", "achVendID"),
    ];
    static ref BOOL_CP: Vec<(&'static str, &'static str, &'static str)> = vec![
        ("isFixedPitch", "post", "isFixedPitch"),
        ("postscriptIsFixedPitch", "post", "isFixedPitch"),
    ];

    // XXX fsType
}

fn load_custom_parameters(ot_values: &mut Vec<OTValue>, params: HashMap<&str, &Plist>) {
    for (key, table, field) in UNSIGNED_CP.iter() {
        if let Some(v) = params.get(key) {
            ot_values.push(OTValue {
                table: table.to_string(),
                field: field.to_string(),
                value: OTScalar::Unsigned((*v).into()),
            });
        }
    }
    for (key, table, field) in SIGNED_CP.iter() {
        if let Some(v) = params.get(key) {
            ot_values.push(OTValue {
                table: table.to_string(),
                field: field.to_string(),
                value: Signed((*v).into()),
            });
        }
    }
    for (key, table, field) in BOOL_CP.iter() {
        if let Some(v) = params.get(key) {
            ot_values.push(OTValue {
                table: table.to_string(),
                field: field.to_string(),
                value: OTScalar::Bool(u32::from(*v) > 0),
            });
        }
    }
}

fn load_instance(font: &mut Font, plist: &PlistDictionary) {
    let location = if plist.contains_key("axesValues") {
        _to_loc(font, plist.get("axesValues"))
    } else {
        unimplemented!()
    };
    let name = plist
        .get("name")
        .and_then(|f| f.string())
        .unwrap_or(&"Unnamed Instance".to_string())
        .to_string();
    let cp = get_custom_parameters(plist);
    if let Some(axis_locs) = cp.get("Axis Location").map(|f| f.iter_array_of_dicts()) {
        for loc in axis_locs {
            let axis_name = loc.get("Axis").and_then(|f| f.string());
            let loc = loc.get("Location").map(f32::from).unwrap_or(0.0);
            if let Some(axis) = font
                .axes
                .iter_mut()
                .find(|ax| ax.name.default().as_ref() == axis_name)
            {
                if let Some(designspace_value) = location.0.get(&axis.tag) {
                    if axis.map.is_none() {
                        axis.map = Some(vec![]);
                    }
                    axis.map.as_mut().unwrap().push((loc, *designspace_value));
                }
            }
        }
    }
    font.instances.push(Instance {
        name: (&name).into(),
        location,
        style_name: (&name).into(),
    });
}

fn fixup_axis_mappings(font: &mut Font) {
    for axis in font.axes.iter_mut() {
        if axis.map.is_none() {
            continue;
        }
        if let Some((min, default, max)) = axis.bounds() {
            axis.min = Some(axis.designspace_to_userspace(min as i32));
            axis.max = Some(axis.designspace_to_userspace(max as i32));
            axis.default = Some(axis.designspace_to_userspace(default as i32));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn do_something() {
        let f = load("data/Nunito3.glyphs".into()).unwrap();
        println!("{:#?}", f);
    }
}