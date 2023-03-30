use clap::{App, Arg};
use fonttools::{tables::C2PA::C2PA};
use fonttools_cli::{open_font, save_font};
use fonttools::tables::C2PA::TAG as c2pa_tag;

fn main() {
    env_logger::init();
    let matches = App::new("ttf-add-c2pa-table")
        .about("Adds a C2PA v0.1 table to a font.")
        .arg(
            Arg::with_name("INPUT")
                .help("Sets the input file to use")
                .required(false),
        )
        .arg(
            Arg::with_name("OUTPUT")
                .help("Sets the output file to use")
                .required(false),
        )
        .arg(
          Arg::with_name("replace")
            .short("x")
            .long("replace")
            .conflicts_with("remove")
            .help("Even if the font contains a C2PA table, replace with new empty table")
            .required(false),
        )
        .arg(
          Arg::with_name("remove")
            .long("remove")
            .short("r")
            .conflicts_with("replace")
            .help("Remove the C2PA table from the font")
            .required(false),
        )
        .arg(
          Arg::with_name("active-manifest-uri")
            .long("active-manifest-uri")
            .short("a")
            .conflicts_with("remove")
            .takes_value(true)
            .help("Optional URI to an active manifest")
            .required(false)
        )
        .get_matches();
    let mut in_font = open_font(&matches);
    let has_c2pa = in_font.tables.contains(&c2pa_tag);
    // if has c2pa and want to remove
    // else if has c2pa and want to
    if has_c2pa && !matches.is_present("replace") && !matches.is_present("remove") {
      log::error!("C2PA table is already present.");
    }
    else if !has_c2pa && matches.is_present("remove") {
      log::error!("C2PA table is not present, nothing to remove");
    }
    else if matches.is_present("remove") {
      in_font.tables.remove(c2pa_tag);
    }
    else {
      let c2pa = C2PA::new(
        matches.value_of("active-manifest-uri").map(|v| v.to_owned()),
        None
      );
      in_font.tables.insert(c2pa);
    }

    save_font(in_font, &matches);
}
