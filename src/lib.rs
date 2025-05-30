//! Small library for getting the CRS from lidar files.
//! Just pass a &las::Header to the parse_las_crs function
//!
//! The library should be able to parse CRS's stored in WKT-CRS v1 and v2 and GeoTiff U16 (E)VLR(s)
//!
//! The CRS is returend in a Result<Crs, CrsError>
//! CRS has the fields horizontal, which is a u16 EPSG code, and vertical, which is an optional u16 EPSG code.
//! Only horizontal CRS's are detected for WKT-CRS (E)VLRs
//! Geotiff-CRS (E)VLRs might have both
//!
//! The validity of the extracted code is not checked.
//! Use the crs-definitions crate for checking validity of EPSG codes.
//!
//! Be aware that certain software adds invalid CRS VLRs when writing CRS-less lidar files (f.ex when QGIS convert .la[s,z] files without a CRS-VLR to .copc.laz files).
//! This is because the las 1.4 spec (which .copc.laz demands), requires a WKT-CRS (E)VLR to be present.
//! These VLRs often contain the invalid EPSG code 0.
//!
//! Userdefined CRS's and CRS's stored in GeoTiff string or Doubles data is not yet supported.
//! The different Error's are described in the CrsError enum

use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Cursor, Seek, SeekFrom};
use thiserror::Error;

use log::{log, Level};

/// crate result
pub type CrsResult<T> = std::result::Result<T, CrsError>;

/// horizontal and optional vertical crs given by EPSG code
#[derive(Debug, Clone, Copy)]
pub struct Crs {
    pub horizontal: u16,
    pub vertical: Option<u16>,
}

/// Error enum
#[derive(Error, Debug)]
pub enum CrsError {
    #[error("The header does not contain any CRS VLRs")]
    NoCrs,
    #[error("Parsing of User Defined CRS not implemented")]
    UserDefinedCrs,
    #[error("Unable to parse the found WKT-CRS (E)VLR")]
    UnreadableWktCrs,
    #[error("Unable to parse the found Geotiff (E)VLR(s)")]
    UnreadableGeotiffCrs,
    #[error("Invalid key for Geotiff data")]
    UndefinedDataForGeoTiffKey(u16),
    #[error("The CRS parser does not handle CRS's defined by Geotiff String and Double data")]
    UnimplementedForGeoTiffStringAndDoubleData(GeoTiffData),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Pass a &las::header to this function to extract the EPSG code of the CRS stored in the CRS (E)VLR(s) if they exist
/// Supports both WKT definitions (by finding the EPSG code on the end of the WKT-string) and
/// GeoTiff (but not GeoTiff string and double defined CRS's, only u16 epsg code)
/// but most CRS's should be detected
pub fn parse_las_crs(header: &las::Header) -> CrsResult<Crs> {
    let mut crs_vlrs = [None, None, None, None];
    for vlr in header.all_vlrs() {
        if let ("lasf_projection", 2112 | 34735 | 34736 | 34737) =
            (vlr.user_id.to_lowercase().as_str(), vlr.record_id)
        {
            let pos = match vlr.record_id {
                2112 => 0,
                34735 => 1,
                34736 => 2,
                34737 => 3,
                _ => unreachable!(),
            };

            crs_vlrs[pos] = Some(&vlr.data);
        }
    }
    let [wkt, geotiff_main, double, string] = crs_vlrs;

    // warn about header and VLR inconsistencies
    if wkt.is_some() && !header.has_wkt_crs() {
        log!(
            Level::Warn,
            "WKT CRS (E)VLR found, but header says it does not exist"
        );
    } else if wkt.is_none() && header.has_wkt_crs() {
        log!(
            Level::Warn,
            "No WKT CRS (E)VLR found, but header says it exists"
        );
    }

    // warn about double defined CRS
    if wkt.is_some() && geotiff_main.is_some() {
        log!(
            Level::Warn,
            "Both WKT and Geotiff CRS (E)VLRs found, WKT is parsed"
        );
    }

    if let Some(wkt) = wkt {
        get_wkt_epsg(wkt)
    } else if let Some(main) = geotiff_main {
        get_geotiff_epsg(main, double, string)
    } else {
        Err(CrsError::NoCrs)
    }
}

/// find the EPSG codes for the WKT string
///
/// split the wkt string in two at VERTCRS
/// and find the horizontal and vertical codes at the end of each substring
fn get_wkt_epsg(bytes: &[u8]) -> CrsResult<Crs> {
    let wkt: String = bytes.iter().map(|b| *b as char).collect();

    // VERT_CS for WKT v1 and VERTCRS for v2
    let pieces = wkt.split_once("VERT");

    let pieces = if let Some((horizontal, vertical)) = pieces {
        // both horizontal and vertical codes exist
        vec![horizontal.as_bytes(), vertical.as_bytes()]
    } else {
        // only horizontal code
        vec![wkt.as_bytes()]
    };

    let mut epsg = [None, None];
    for (pi, piece) in pieces.into_iter().enumerate() {
        let mut epsg_code = 0;
        let mut has_code_started = false;
        let mut power = 0;
        for (i, byte) in piece.iter().rev().enumerate() {
            if (48..=57).contains(byte) {
                // the byte is an ASCII encoded number
                has_code_started = true;

                epsg_code += 10_u16.pow(power) * (byte - 48) as u16;
                power += 1;
            } else if has_code_started {
                break;
            }
            if i > 7 {
                break;
            }
        }
        if epsg_code != 0 {
            epsg[pi] = Some(epsg_code);
        }
    }
    if epsg[0].is_none() {
        return Err(CrsError::UnreadableWktCrs);
    }

    Ok(Crs {
        horizontal: epsg[0].unwrap(),
        vertical: epsg[1],
    })
}

/// Gets the EPSG code in the geotiff crs vlrs
/// returns a tuple containing the horizontal code and the optional vertical code
fn get_geotiff_epsg(
    main_vlr: &[u8],
    double_vlr: Option<&Vec<u8>>,
    ascii_vlr: Option<&Vec<u8>>,
) -> CrsResult<Crs> {
    let mut main_vlr = Cursor::new(main_vlr);

    main_vlr.read_u16::<LittleEndian>()?; // always 1
    main_vlr.read_u16::<LittleEndian>()?; // always 1
    main_vlr.read_u16::<LittleEndian>()?; // always 0
    let num_keys = main_vlr.read_u16::<LittleEndian>()?;

    let crs_data = GeoTiffCRS::read_from(main_vlr, double_vlr, ascii_vlr, num_keys)?;

    let mut out = (None, None);
    for entry in crs_data.entries {
        match entry.id {
            // 3072 and 2048 should not co-exist, but might both be combined with 4096
            // 1024 should always exist
            1024 => match entry.data {
                GeoTiffData::U16(0) => return Err(CrsError::UnreadableGeotiffCrs),
                GeoTiffData::U16(1) => (), // projected crs
                GeoTiffData::U16(2) => (), // geographic coordinates
                GeoTiffData::U16(3) => (), // geographic + a vertical crs
                GeoTiffData::U16(32_767) => return Err(CrsError::UserDefinedCrs),
                p => return Err(CrsError::UnimplementedForGeoTiffStringAndDoubleData(p)),
            },
            2048 | 3072 => {
                if let GeoTiffData::U16(v) = entry.data {
                    out.0 = Some(v);
                } else {
                    // should probably add support for this
                    return Err(CrsError::UndefinedDataForGeoTiffKey(entry.id));
                }
            }
            4096 => {
                // vertical crs
                if let GeoTiffData::U16(v) = entry.data {
                    out.1 = Some(v);
                } else {
                    // should probably add support for this
                    return Err(CrsError::UndefinedDataForGeoTiffKey(4096));
                }
            }
            _ => (), // the rest are descriptions and units.
        }
    }
    if out.0.is_none() {
        return Err(CrsError::UnreadableGeotiffCrs);
    }
    Ok(Crs {
        horizontal: out.0.unwrap(),
        vertical: out.1,
    })
}

#[derive(Debug)]
pub struct GeoTiffCRS {
    pub entries: Vec<GeoTiffKeyEntry>,
}

impl GeoTiffCRS {
    fn read_from(
        mut main_vlr: Cursor<&[u8]>,
        double_vlr: Option<&Vec<u8>>,
        ascii_vlr: Option<&Vec<u8>>,
        count: u16,
    ) -> CrsResult<Self> {
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            entries.push(GeoTiffKeyEntry::read_from(
                &mut main_vlr,
                &double_vlr,
                &ascii_vlr,
            )?);
        }
        Ok(GeoTiffCRS { entries })
    }
}

#[derive(Debug)]
pub enum GeoTiffData {
    U16(u16),
    String(String),
    Doubles(Vec<f64>),
}

#[derive(Debug)]
pub struct GeoTiffKeyEntry {
    id: u16,
    data: GeoTiffData,
}

impl GeoTiffKeyEntry {
    fn read_from(
        main_vlr: &mut Cursor<&[u8]>,
        double_vlr: &Option<&Vec<u8>>,
        ascii_vlr: &Option<&Vec<u8>>,
    ) -> CrsResult<Self> {
        let id = main_vlr.read_u16::<LittleEndian>()?;
        let location = main_vlr.read_u16::<LittleEndian>()?;
        let count = main_vlr.read_u16::<LittleEndian>()?;
        let offset = main_vlr.read_u16::<LittleEndian>()?;
        let data = match location {
            0 => GeoTiffData::U16(offset),
            34736 => {
                let mut cursor = Cursor::new(double_vlr.ok_or(CrsError::UnreadableGeotiffCrs)?);
                cursor.seek(SeekFrom::Start(offset as u64 * 8_u64))?; // 8 is the byte size of a f64 and offset is not a byte offset but an index
                let mut doubles = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    doubles.push(cursor.read_f64::<LittleEndian>()?);
                }
                GeoTiffData::Doubles(doubles)
            }
            34737 => {
                let mut cursor = Cursor::new(ascii_vlr.ok_or(CrsError::UnreadableGeotiffCrs)?);
                cursor.seek(SeekFrom::Start(offset as u64))?; // no need to multiply the index as the byte size of char is 1
                let mut string = String::with_capacity(count as usize);
                for _ in 0..count {
                    string.push(cursor.read_u8()? as char);
                }
                GeoTiffData::String(string)
            }
            _ => return Err(CrsError::UndefinedDataForGeoTiffKey(id)),
        };
        Ok(GeoTiffKeyEntry { id, data })
    }
}
