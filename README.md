Small library for getting the CRS in the form of a EPSG code from lidar files.

Either call `ParseEpsgCRS::get_epsg_crs` on a `las::Header` or use one of
`get_epsg_from_geotiff_crs` or `get_epsg_from_wkt_crs_bytes` on the output
of `las::Header::get_geotiff_crs` or `las::Header::get_wkt_crs_bytes` respectively

The library should be able to parse CRS's stored in WKT-CRS v1 and v2 and GeoTiff U16 (E)VLR(s) stored in both las and laz files (with the laz feature flag activated).

The CRS is returend in a `Result<EpsgCRS, crate::Error>`. \
`EpsgCRS` has the fields horizontal, which is a `u16` EPSG code, and vertical, which is an `Option<u16>` EPSG code. \
If a parsed vertical code is outside `EPSG_RANGE` it is ignored and set to `None`. \
If a parsed horizontal code is outside `EPSG_RANGE` an `Err(Error::BadHorizontalCodeParsed(EpsgCRS))` is returned.

The validity of the extracted code is only checked against being in `EPSG_RANGE`. \
Use the [crs-definitions](https://docs.rs/crs-definitions/latest/crs_definitions/) crate for checking against a registry of valid horizontal EPSG codes.

Be aware that certain software adds invalid CRS VLRs when writing CRS-less lidar files (f.ex when QGIS convert .la(s,z) files without a CRS-VLR to .copc.laz files).
This is because the las 1.4 spec (which .copc.laz demands), requires a WKT-CRS (E)VLR to be present (or more generally, all lidar files are supposed to contain CRS data).
These VLRs often contain the invalid EPSG code 0 and trying to extract that code will return a BadHorizontalCodeParsed Error.

Parsing EPSG codes from user-defined CRS's and CRS's stored in GeoTiff Ascii or Double data is not supported.
But the relevant `las::crs::GeoTiffData` is returned with the `Error::UnimplementedForGeoTiffStringAndDoubleData(las::crs::GeoTiffData)`. \
If you have a Lidar file with CRS defined in this way please make an issue on Github so I can take a look at parsing them.
I have yet to see a Lidar file with CRS defined in that way.