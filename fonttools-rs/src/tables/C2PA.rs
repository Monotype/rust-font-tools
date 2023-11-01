/**
 * @copyright 2023 Monotype Imaging Inc.
 *
 * @file C2PA.rs
 *
 * @brief C2PA OpenType extension font table
 *
 */
use otspec::types::*;
use otspec::{
    DeserializationError, Deserialize, Deserializer, ReaderContext, SerializationError, Serialize,
};
use otspec_macros::tables;
use std::str;

/// The 'C2PA' OpenType extension tag.
pub const TAG: Tag = crate::tag!("C2PA");

// Defines the internal C2PA header record
tables!(
    C2PARecordInternal {
        uint16 majorVersion
        uint16 minorVersion
        u32 activeManifestUriOffset
        uint16 activeManifestUriLength
        uint16 reserved
        u32 manifestStoreOffset
        u32 manifestStoreLength
    }
);

/// A C2PA record, containing information about a current active manifest and/or
/// an embedded manifest store.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(non_snake_case)]
pub struct C2PA {
    /// Major version of the C2PA table record
    pub majorVersion: uint16,
    /// Minor version of the C2PA table record
    pub minorVersion: uint16,
    /// Optional URI to an active manifest
    pub activeManifestUri: Option<String>,
    /// Optional embedded manifest store
    manifestStore: Option<Box<[u8]>>,
}

impl C2PA {
    /// Creates a new C2PA record with the current default version information.
    pub fn new(active_manifest_uri: Option<String>, manifest_store: Option<Vec<u8>>) -> Self {
        let boxed_data = manifest_store.map(|d| d.into_boxed_slice());
        Self {
            activeManifestUri: active_manifest_uri,
            manifestStore: boxed_data,
            ..C2PA::default()
        }
    }

    /// Get the manifest store data if available
    pub fn get_manifest_store(&self) -> Option<&[u8]> {
        self.manifestStore.as_deref()
    }
}

impl Default for C2PA {
    fn default() -> Self {
        Self {
            majorVersion: 0,
            minorVersion: 1,
            activeManifestUri: Default::default(),
            manifestStore: Default::default(),
        }
    }
}

impl Deserialize for C2PA {
    fn from_bytes(c: &mut ReaderContext) -> Result<Self, DeserializationError> {
        let mut active_manifest_uri: Option<String> = None;
        let mut manifest_store: Option<Box<[u8]>> = None;
        // Save the pointer of the current reader context, before we read the
        // internal record for obtaining the offset from the beginning of the
        // table to the data as to specification.
        c.push();

        // Read the components of the C2PA header
        let internal_record: C2PARecordInternal = c.de()?;

        if internal_record.activeManifestUriOffset > 0 {
            // Offset to the active manifest URI
            c.ptr = c.top_of_table() + internal_record.activeManifestUriOffset as usize;
            // Reading in the active URI as bytes
            let uri_as_bytes: Vec<u8> =
                c.de_counted(internal_record.activeManifestUriLength as usize)?;
            // And converting to a string read as UTF-8 encoding
            active_manifest_uri = Some(
                str::from_utf8(&uri_as_bytes)
                    .map_err(|_| {
                        DeserializationError("Failed to read UTF-8 string from bytes".to_string())
                    })?
                    .to_string(),
            );
        }

        if internal_record.manifestStoreOffset > 0 {
            // Reset the offset to the C2PA manifest store
            c.ptr = c.top_of_table() + internal_record.manifestStoreOffset as usize;
            // Read the store as bytes
            let store_as_bytes: Option<Vec<u8>> =
                Some(c.de_counted(internal_record.manifestStoreLength as usize)?);
            // And then convert to a string as UTF-8 bytes
            manifest_store = store_as_bytes.map(|d| d.into_boxed_slice());
        }

        // Restore the state of the reader
        c.pop();

        // Return our record
        Ok(C2PA {
            majorVersion: internal_record.majorVersion,
            minorVersion: internal_record.minorVersion,
            activeManifestUri: active_manifest_uri,
            manifestStore: manifest_store,
        })
    }
}

impl Serialize for C2PA {
    fn to_bytes(&self, data: &mut Vec<u8>) -> Result<(), SerializationError> {
        // The main offset to the data includes the major/minor versions,
        // the offset/length of the active manifest uri, two reserved bytes,
        // and the the offset/length of the C2PA manifest store data.
        let offset: u32 = 20;
        // Create a data pool for the C2PA data
        let mut c2pa_data_pool: Vec<u8> = Vec::new();

        // The active manifest is optional, so default to 0 for offset/length
        let mut active_manifest_offset: u32 = 0_u32;
        let mut active_manifest_length: u16 = 0_u16;

        // But if we have a valid active manifest URI, we will use the real
        // values
        if let Some(val) = self.activeManifestUri.as_ref() {
            active_manifest_offset = offset;
            active_manifest_length = val.len() as u16;
            // Add the data to the data pool
            c2pa_data_pool.extend(val.as_bytes());
        }

        // An embedded manifest store is optional, so default to 0 for
        // offset/length
        let mut manifest_store_offset: u32 = 0_u32;
        let mut manifest_store_length: u32 = 0_u32;
        // Again, if we do have data for an embedded manifest store, we will use
        // the real values.
        if let Some(val) = self.manifestStore.as_ref() {
            manifest_store_offset = offset + active_manifest_length as u32;
            manifest_store_length = val.len() as u32;
            // Adding the data to the data pool to write at the end of the table
            // entry
            c2pa_data_pool.extend(val.iter());
        }

        // At this point, we have everything we need to build the C2PA header
        // record
        let c2pa_internal_record = C2PARecordInternal {
            majorVersion: self.majorVersion,
            minorVersion: self.minorVersion,
            activeManifestUriOffset: active_manifest_offset,
            activeManifestUriLength: active_manifest_length,
            reserved: 0,
            manifestStoreOffset: manifest_store_offset,
            manifestStoreLength: manifest_store_length,
        };
        // All that is left is to write the header and data to the buffer
        c2pa_internal_record.to_bytes(data)?;
        c2pa_data_pool.to_bytes(data)
    }
}

#[cfg(test)]
mod tests {
    /// Verifies the behavior when the activeManifestUri is None
    #[test]
    fn c2pa_none_uri() {
        let c2pa = super::C2PA {
            majorVersion: 0,
            minorVersion: 1,
            activeManifestUri: None,
            manifestStore: Some(vec![0x74, 0x65, 0x73, 0x74, 0x2D, 0x64, 0x61, 0x74, 0x61].into()),
        };
        let binary_c2pa = vec![
            0x00, 0x00, // Major version
            0x00, 0x01, // Minor version
            0x00, 0x00, 0x00, 0x00, // Active manifest URI offset
            0x00, 0x00, // Active manifest URI length
            0x00, 0x00, // Two reserved bytes - should be zero, but should also be ignored/passed-through
            0x00, 0x00, 0x00, 0x14, // C2PA manifest store offset
            0x00, 0x00, 0x00, 0x09, // C2PA manifest store length
            0x74, 0x65, 0x73, 0x74, 0x2D, 0x64, 0x61, 0x74, 0x61, // C2PA manifest store data
        ];
        let deserialized: super::C2PA = otspec::de::from_bytes(&binary_c2pa).unwrap();
        assert_eq!(deserialized, c2pa);
        let serialized = otspec::ser::to_bytes(&deserialized).unwrap();
        assert_eq!(serialized, binary_c2pa);
    }

    /// Verifies the behavior when the manifestStore is None
    #[test]
    fn c2pa_none_manifest_store() {
        let c2pa = super::C2PA {
            majorVersion: 0,
            minorVersion: 1,
            activeManifestUri: Some("file://a".to_owned()),
            manifestStore: None,
        };
        let binary_c2pa = vec![
            0x00, 0x00, // Major version
            0x00, 0x01, // Minor version
            0x00, 0x00, 0x00, 0x14, // Active manifest URI offset
            0x00, 0x08, // Active manifest URI length
            0x00, 0x00, // Two reserved bytes - should be zero, but should also be ignored/passed-through
            0x00, 0x00, 0x00, 0x00, // C2PA manifest store offset
            0x00, 0x00, 0x00, 0x00, // C2PA manifest store length
            0x66, 0x69, 0x6C, 0x65, 0x3A, 0x2F, 0x2F, 0x61, // active manifest uri data
        ];
        let deserialized: super::C2PA = otspec::de::from_bytes(&binary_c2pa).unwrap();
        assert_eq!(deserialized, c2pa);
        let serialized = otspec::ser::to_bytes(&deserialized).unwrap();
        assert_eq!(serialized, binary_c2pa);
    }

    /// Verifies the behavior when there is both an active manifest URI and a
    /// C2PA manifest store in the font.
    #[test]
    fn c2pa_otspec() {
        let c2pa = super::C2PA {
            majorVersion: 0,
            minorVersion: 1,
            activeManifestUri: Some("file://a".to_owned()),
            manifestStore: Some(vec![0x74, 0x65, 0x73, 0x74, 0x2D, 0x64, 0x61, 0x74, 0x61].into()),
        };
        let binary_c2pa = vec![
            0x00, 0x00, // Major version
            0x00, 0x01, // Minor version
            0x00, 0x00, 0x00, 0x14, // Active manifest URI offset
            0x00, 0x08, // Active manifest URI length
            0x00, 0x00, // Two reserved bytes - should be zero, but should also be ignored/passed-through
            0x00, 0x00, 0x00, 0x1C, // C2PA manifest store offset
            0x00, 0x00, 0x00, 0x09, // C2PA manifest store length
            0x66, 0x69, 0x6C, 0x65, 0x3A, 0x2F, 0x2F, 0x61, // active manifest uri data
            0x74, 0x65, 0x73, 0x74, 0x2D, 0x64, 0x61, 0x74, 0x61, // C2PA manifest store data
        ];
        let deserialized: super::C2PA = otspec::de::from_bytes(&binary_c2pa).unwrap();
        assert_eq!(deserialized, c2pa);
        let serialized = otspec::ser::to_bytes(&deserialized).unwrap();
        assert_eq!(serialized, binary_c2pa);
    }
}
