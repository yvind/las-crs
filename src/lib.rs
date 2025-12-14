//! Small library for getting the CRS in the form of a EPSG code from lidar files.
//! Either call [ParseEpsgCRS::get_epsg_crs] on a [las::Header] or use one of
//! [get_epsg_from_geotiff_crs] or [get_epsg_from_wkt_crs_bytes] on the output
//! of [las::Header::get_geotiff_crs] or [las::Header::get_wkt_crs_bytes] respectively
//!
//! The library should be able to parse CRS's stored in WKT-CRS v1 and v2 and GeoTiff U16 (E)VLR(s) stored in both las and laz files (with the laz feature flag activated).
//!
//! The CRS is returend in a `Result<EpsgCRS, crate::Error>`
//! CRS has the fields horizontal, which is a u16 EPSG code, and vertical, which is an optional u16 EPSG code.
//! If a parsed vertical code is outside [EPSG_RANGE] it is ignored and set to `None`.
//! If a parsed horizontal code is outside [EPSG_RANGE] an `Err(Error::BadHorizontalCodeParsed(EpsgCRS))` is returned
//!
//! The validity of the extracted code is only checked against being in [EPSG_RANGE].
//! Use the [crs-definitions](https://docs.rs/crs-definitions/latest/crs_definitions/) crate for checking validity of EPSG codes.
//!
//! Be aware that certain software adds invalid CRS VLRs when writing CRS-less lidar files (f.ex when QGIS convert .la[s,z] files without a CRS-VLR to .copc.laz files).
//! This is because the las 1.4 spec (which .copc.laz demands), requires a WKT-CRS (E)VLR to be present.
//! These VLRs often contain the invalid EPSG code 0 and trying to extract that code will return a BadHorizontalCodeParsed Error.
//!
//! Parsing EPSG codes from user-defined CRS's and CRS's stored in GeoTiff String or Double data is not supported.
//! But the relevant [las::crs::GeoTiffData] is returned with the `Error::UnimplementedForGeoTiffStringAndDoubleData(las::crs::GeoTiffData)`
//! If you have a Lidar file with CRS defined in this way please make an issue on Github so I can create tests for it
//! I have yet to see a Lidar file with CRS defined in that way

use las::{
    Header,
    crs::{GeoTiffCrs, GeoTiffData},
};
use log::{Level, log};
use thiserror::Error;

type Result<T> = std::result::Result<T, Error>;

pub const EPSG_RANGE: std::ops::RangeInclusive<u16> = 1024..=(i16::MAX as u16);

/// Horizontal and optional vertical CRS given by EPSG code(s)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EpsgCRS {
    /// EPSG code for the horizontal CRS
    horizontal: u16,

    /// Optional EPSG code for the vertical CRS
    vertical: Option<u16>,
}

impl EpsgCRS {
    /// Construct a new EpsgCrs both components are checked against EPSG_RANGE
    pub fn new(horizontal_code: u16, vertical_code: Option<u16>) -> Result<Self> {
        let code = EpsgCRS {
            horizontal: horizontal_code,
            vertical: vertical_code,
        };
        if code.in_epsg_range() {
            Ok(code)
        } else {
            Err(Error::BadEPSGCrs)
        }
    }

    /// Construct a new EpsgCrs neither component is checked against EPSG_RANGE
    pub fn new_unchecked(horizontal_code: u16, vertical_code: Option<u16>) -> Self {
        EpsgCRS {
            horizontal: horizontal_code,
            vertical: vertical_code,
        }
    }

    /// Checked both components against EPSG_RANGE
    pub fn in_epsg_range(&self) -> bool {
        if let Some(vc) = &self.vertical
            && !EPSG_RANGE.contains(vc)
        {
            return false;
        }
        EPSG_RANGE.contains(&self.horizontal)
    }

    /// get the horizontal code
    pub fn get_horizontal(&self) -> u16 {
        self.horizontal
    }

    /// get the optional vertical code
    pub fn get_vertical(&self) -> Option<u16> {
        self.vertical
    }

    /// set the horizontal code, the new code is checked against EPSG_RANGE before setting
    pub fn set_horizontal(&mut self, horizontal_code: u16) -> Result<()> {
        if EPSG_RANGE.contains(&horizontal_code) {
            self.horizontal = horizontal_code;
            Ok(())
        } else {
            Err(Error::SetBadCode(horizontal_code))
        }
    }

    /// set the vertical code, the new code is checked against EPSG_RANGE before setting
    pub fn set_vertical(&mut self, vertical_code: u16) -> Result<()> {
        if EPSG_RANGE.contains(&vertical_code) {
            self.vertical = Some(vertical_code);
            Ok(())
        } else {
            Err(Error::SetBadCode(vertical_code))
        }
    }

    /// set the horizontal code without checking against EPSG_RANGE
    pub fn set_horizontal_unchecked(&mut self, horizontal_code: u16) {
        self.horizontal = horizontal_code;
    }

    /// set the vertical code without checking against EPSG_RANGE
    pub fn set_vertical_unchecked(&mut self, vertical_code: u16) {
        self.vertical = Some(vertical_code)
    }
}

/// Error enum
#[derive(Error, Debug)]
pub enum Error {
    /// Error propagated from the Las lib
    #[error(transparent)]
    LasError(#[from] las::Error),
    /// User defined CRS cannot be parsed
    #[error("Parsing of User Defined CRS not implemented")]
    UserDefinedCrs,
    /// Could not parse the CRS from the WKT-data
    #[error("Unable to parse the found WKT-CRS (E)VLR")]
    UnreadableWktCrs,
    /// The parsed horizontal code is outside of [EPSG_RANGE]
    #[error("The parsed code for the horizontal component is outside of the EPSG-range")]
    BadHorizontalCodeParsed(EpsgCRS),
    /// Cannot parse EPSG code from ascii string or double data
    #[error("The CRS parser does not handle CRS's defined by Geotiff String and Double data")]
    UnimplementedForGeoTiffStringAndDoubleData(GeoTiffData),
    /// The provided code for setting is outside of EPSG_RANGE
    #[error("The provided code for setting is outside of EPSG_RANGE")]
    SetBadCode(u16),
    /// The EPSG CRS is outside of EPSG_RANGE
    #[error("A component of the EPSG code is outside of EPSG_RANGE")]
    BadEPSGCrs,
}

pub trait ParseEpsgCRS {
    fn get_epsg_crs(&self) -> Result<Option<EpsgCRS>>;
}

impl ParseEpsgCRS for Header {
    /// Parse the EPSG coordinate reference system (CRSes) code(s) from the header.
    ///
    /// Las stores CRS-info in (E)VLRs either as Well Known Text (WKT) or in GeoTIff-format
    /// **Most**, but not all, CRS' used for Aerial Lidar has an associated EPSG code.
    /// Use this function to try and parse the EPSG code(s) from the header.
    ///
    /// WKT takes precedence over GeoTiff in this function, but they should not co-exist.
    ///
    /// Just because this function fails does not mean that no CRS-data is available.
    /// Use functions [Self::get_wkt_crs_bytes] or [Self::get_geotiff_crs] to get all data stored in the CRS-(E)VLRs.
    ///
    /// Parsing code(s) from WKT-CRS v1 or v2 and GeoTiff U16-data is supported.
    ///
    /// The validity of the extracted code is not checked, beyond checking that it is in [EPSG_RANGE].
    /// Use the [crs-definitions](https://docs.rs/crs-definitions/latest/crs_definitions/) crate for checking the validity of a horizontal EPSG code.
    ///
    /// # Example
    ///
    /// ```
    /// use las::Reader;
    /// use las_crs::ParseEpsgCRS;
    ///
    /// let reader = Reader::from_path("testdata/autzen.las").expect("Cannot open reader");
    /// let epsg = reader.header().get_epsg_crs().expect("Cannot parse EPSG code(s) from the CRS-(E)VLR(s)").expect("The Lidar file had no CRS");
    /// ```
    fn get_epsg_crs(&self) -> Result<Option<EpsgCRS>> {
        if let Some(wkt) = self.get_wkt_crs_bytes() {
            if !self.has_wkt_crs() {
                log!(
                    Level::Warn,
                    "WKT CRS (E)VLR found, but header says it does not exist"
                );
            }
            Ok(Some(get_epsg_from_wkt_crs_bytes(wkt)?))
        } else if let Some(geotiff) = self.get_geotiff_crs()? {
            if self.has_wkt_crs() {
                log!(
                    Level::Warn,
                    "Only Geotiff CRS (E)VLR(s) found, but header says WKT exists"
                );
            }
            Ok(Some(get_epsg_from_geotiff_crs(&geotiff)?))
        } else {
            if self.has_wkt_crs() {
                log!(
                    Level::Warn,
                    "No CRS (E)VLR(s) found, but header says WKT exists"
                );
            }
            Ok(None)
        }
    }
}

/// Tries to parse EPSG code(s) from WKT-CRS bytes.
///
/// By parsing the EPSG codes at the end of the vertical and horizontal CRS sub-strings
/// This is not true WKT parser and might provide a bad code if
/// the WKT-CRS bytes does not look as expected
pub fn get_epsg_from_wkt_crs_bytes(bytes: &[u8]) -> Result<EpsgCRS> {
    let wkt = String::from_utf8_lossy(bytes);

    enum WktPieces<'a> {
        One(&'a [u8]),
        Two(&'a [u8], &'a [u8]),
    }

    impl WktPieces<'_> {
        fn parse_codes(&self) -> (u16, u16) {
            match self {
                WktPieces::One(hor) => (Self::get_code(hor), 0),
                WktPieces::Two(hor, ver) => (Self::get_code(hor), Self::get_code(ver)),
            }
        }

        fn get_code(bytes: &[u8]) -> u16 {
            // the EPSG code is located at the end of the substrings
            // and so we iterate through the substrings backwards collecting
            // digits and adding them to our EPSG code
            let mut epsg_code = 0;
            let mut code_has_started = false;
            let mut power = 1;
            // the 10 last bytes should be enough (with a small margin)
            // as the code is 4 or 5 digits starting at the 2nd or 3rd byte from the back
            for byte in bytes.trim_ascii_end().iter().rev().take(10) {
                // if the byte is an ASCII encoded digit
                if byte.is_ascii_digit() {
                    // mark that the EPSG code has started
                    // so that we can break when we no
                    // longer find digits
                    code_has_started = true;

                    // translate from ASCII to digits
                    // and multiply by powers of 10
                    // sum it to build the EPSG
                    // code digit by digit
                    epsg_code += power * (byte - 48) as u16;
                    power *= 10;
                } else if code_has_started {
                    // we no longer see digits
                    // so the code must be over
                    break;
                }
            }
            epsg_code
        }
    }

    // VERT_CS for WKT v1 and VERTCRS or VERTICALCRS for v2
    let pieces = if let Some((horizontal, vertical)) = wkt.split_once("VERTCRS") {
        WktPieces::Two(horizontal.as_bytes(), vertical.as_bytes())
    } else if let Some((horizontal, vertical)) = wkt.split_once("VERTICALCRS") {
        WktPieces::Two(horizontal.as_bytes(), vertical.as_bytes())
    } else if let Some((horizontal, vertical)) = wkt.split_once("VERT_CS") {
        WktPieces::Two(horizontal.as_bytes(), vertical.as_bytes())
    } else {
        WktPieces::One(wkt.as_bytes())
    };

    let codes = pieces.parse_codes();
    let mut code = EpsgCRS {
        horizontal: codes.0,
        vertical: Some(codes.1),
    };

    if !EPSG_RANGE.contains(&code.horizontal) {
        return Err(Error::BadHorizontalCodeParsed(code));
    }
    if let Some(v_code) = code.vertical
        && !EPSG_RANGE.contains(&v_code)
    {
        code.vertical = None;
    }
    Ok(code)
}

/// Get the EPSG code(s) from GeoTiff-CRS-data
/// Only handles geotiff u16 data
/// Returns ascii and double defined crs data in an [Error::UnimplementedForGeoTiffStringAndDoubleData]
pub fn get_epsg_from_geotiff_crs(geotiff_crs_data: &GeoTiffCrs) -> Result<EpsgCRS> {
    let mut out = (0, None);
    for entry in geotiff_crs_data.entries.iter() {
        match entry.id {
            // 2048 and 3072 should not co-exist, but might both be combined with 4096
            // 1024 should always exist
            1024 => match &entry.data {
                GeoTiffData::U16(0) => (), // should really not be zero, but let's rather error out later just in case
                GeoTiffData::U16(1) => (), // projected crs
                GeoTiffData::U16(2) => (), // geographic crs
                GeoTiffData::U16(3) => (), // geographic + a vertical crs
                GeoTiffData::U16(32_767) => return Err(Error::UserDefinedCrs),
                _ => {
                    return Err(Error::UnimplementedForGeoTiffStringAndDoubleData(
                        entry.data.clone(),
                    ));
                }
            },
            2048 | 3072 => {
                if let GeoTiffData::U16(v) = entry.data {
                    out.0 = v;
                }
            }
            4096 => {
                // vertical crs
                if let GeoTiffData::U16(v) = entry.data {
                    out.1 = Some(v);
                }
            }
            _ => (), // the rest are descriptions and units.
        }
    }

    if out.0 == 0 {
        Err(las::Error::UnreadableGeoTiffCrs)?
    }

    let mut code = EpsgCRS {
        horizontal: out.0,
        vertical: out.1,
    };

    if !EPSG_RANGE.contains(&code.horizontal) {
        return Err(Error::BadHorizontalCodeParsed(code));
    }
    if let Some(v_code) = code.vertical
        && !EPSG_RANGE.contains(&v_code)
    {
        code.vertical = None;
    }
    Ok(code)
}

#[cfg(test)]
mod tests {
    use crate::ParseEpsgCRS;
    use las::Reader;

    #[test]
    fn test_get_epsg_crs_wkt_vlr_autzen() {
        let reader = Reader::from_path("testdata/autzen.copc.laz").expect("Cannot open reader");
        let crs = reader
            .header()
            .get_epsg_crs()
            .expect("Could not get epsg code")
            .expect("The found EPSG was None");

        assert!(crs.horizontal == 2992);
        assert!(crs.vertical == Some(6360))
    }

    #[test]
    fn test_get_epsg_crs_geotiff_vlr_norway() {
        let reader = Reader::from_path("testdata/32-1-472-150-76.laz").expect("Cannot open reader");
        let crs = reader.header().get_epsg_crs().unwrap().unwrap();
        assert!(crs.horizontal == 25832);
        assert!(crs.vertical == Some(5941));
    }

    #[test]
    fn test_get_epsg_crs_wkt_vlr_autzen_las() {
        let reader = Reader::from_path("testdata/autzen.las").expect("Cannot open reader");
        let crs = reader
            .header()
            .get_epsg_crs()
            .expect("Could not get epsg code")
            .expect("The found EPSG was None");

        assert!(crs.horizontal == 2994);
        assert!(crs.vertical.is_none())
    }
}
