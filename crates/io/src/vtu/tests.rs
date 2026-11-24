use std::io::Write;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use flate2::{Compression, write::ZlibEncoder};

use super::{collection, read, single_piece};

const POINTS: &str = r#"<DataArray type="Float64" NumberOfComponents="3" format="ascii">0 0 0 1 0 0 0 1 0 0 0 0</DataArray>"#;
const POINT_FIELD: &str = r#"<DataArray type="Float32" Name="E" NumberOfComponents="2" format="ascii">0 1 2 3 4 5 6 7</DataArray>"#;

fn grid(points: &str, field: &str, appended: &[u8], options: &str) -> Vec<u8> {
    let mut file = format!(
        r#"<VTKFile type="UnstructuredGrid" {options}><UnstructuredGrid><Piece NumberOfPoints="4" NumberOfCells="2"><Points>{points}</Points><Cells>
          <DataArray type="Int32" Name="connectivity">0 1 2 3 1 2</DataArray>
          <DataArray type="Int64" Name="offsets">3 6</DataArray><DataArray type="UInt8" Name="types">5 5</DataArray>
          </Cells><PointData>{field}</PointData><CellData><DataArray type="Float64" Name="S">7 -2</DataArray></CellData></Piece></UnstructuredGrid>"#
    )
    .into_bytes();
    file.extend_from_slice(appended);
    file.extend_from_slice(b"</VTKFile>");
    file
}

#[test]
fn point_ids_and_both_field_associations_survive() {
    let frame = read(&grid(POINTS, POINT_FIELD, b"", ""), 0.25).unwrap();
    assert_eq!(frame.time, 0.25);
    assert_eq!(frame.grid.points.len(), 4);
    assert_eq!(frame.grid.points[0], frame.grid.points[3]);
    assert_eq!(frame.grid.cells[1].points, [3, 1, 2]);
    assert_eq!(frame.point_data["E"].components, 2);
    assert_eq!(frame.point_data["E"].values, [0., 1., 2., 3., 4., 5., 6., 7.]);
    assert_eq!(frame.cell_data["S"].values, [7., -2.]);
}

#[test]
fn appended_raw_32_and_64_bit_headers_and_both_byte_orders() {
    for (wide, little) in [(false, true), (true, false)] {
        let options = if little { r#"byte_order="LittleEndian""# } else { r#"byte_order="BigEndian" header_type="UInt64""# };
        let mut block = b"<AppendedData encoding=\"raw\">\n_".to_vec();
        if wide {
            block.extend_from_slice(&32_u64.to_be_bytes());
        } else {
            block.extend_from_slice(&32_u32.to_le_bytes());
        }
        for x in [0_f32, 1., 2., 3., 4., 5., 6., -7.] {
            block.extend_from_slice(&if little { x.to_le_bytes() } else { x.to_be_bytes() });
        }
        block.extend_from_slice(b"\n</AppendedData>");
        let field = r#"<DataArray type="Float32" Name="E" NumberOfComponents="2" format="appended" offset="0"/>"#;
        let frame = read(&grid(POINTS, field, &block, options), 0.).unwrap();
        assert_eq!(frame.point_data["E"].values.last(), Some(&-7.));
        assert!(read(&grid(POINTS, &field.replace("offset=\"0", "offset=\"1"), &block, options), 0.).is_err());
    }
}

#[test]
fn inline_zlib_with_separately_padded_header_and_multiple_blocks() {
    let raw: Vec<u8> = (0..8).flat_map(|x| (x as f32).to_le_bytes()).collect();
    let blocks: Vec<Vec<u8>> = raw
        .chunks(10)
        .map(|b| {
            let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
            z.write_all(b).unwrap();
            z.finish().unwrap()
        })
        .collect();
    let mut header: Vec<u8> = [4_u32, 10, 2].into_iter().flat_map(u32::to_le_bytes).collect();
    for b in &blocks {
        header.extend_from_slice(&(b.len() as u32).to_le_bytes());
    }
    let data = format!("{}\n{}", STANDARD.encode(header), STANDARD.encode(blocks.concat()));
    let field = format!(r#"<DataArray type="Float32" Name="E" NumberOfComponents="2" format="binary">{data}</DataArray>"#);
    let opts = r#"compressor="vtkZLibDataCompressor""#;
    assert_eq!(read(&grid(POINTS, &field, b"", opts), 0.).unwrap().point_data["E"].values[7], 7.);
    // A declared expansion that disagrees with the grid is rejected before inflation.
    let malformed = field.replace("NumberOfComponents=\"2\"", "NumberOfComponents=\"3\"");
    assert!(read(&grid(POINTS, &malformed, b"", opts), 0.).is_err());
}

#[test]
fn appended_base64_offsets_refer_to_encoded_bytes() {
    let mut raw = 32_u32.to_le_bytes().to_vec();
    raw.extend((0..8).flat_map(|x| (x as f32).to_le_bytes()));
    let inline = STANDARD.encode(&raw);
    let prefix = STANDARD.encode(0_u32.to_le_bytes());
    let appended = format!(r#"<AppendedData encoding="base64">_{prefix}{inline}</AppendedData>"#);
    let field = format!(r#"<DataArray type="Float32" Name="E" NumberOfComponents="2" format="appended" offset="{}"/>"#, prefix.len());
    assert_eq!(read(&grid(POINTS, &field, appended.as_bytes(), ""), 0.).unwrap().point_data["E"].values[7], 7.);
}

#[test]
fn invalid_grids_report_the_cause() {
    let good = String::from_utf8(grid(POINTS, POINT_FIELD, b"", "")).unwrap();
    for (from, to, message) in [
        ("3 6", "3 5", "offsets"),
        ("3 1 2", "4 1 2", "missing point"),
        ("5 5", "5 200", "cell type 200"),
        ("0 1 2 3 4 5 6 7", "0 1 2", "too few"),
        ("NumberOfPoints=\"4\"", "NumberOfPoints=\"9999999999\"", "NumberOfPoints"),
        ("0 1 2 3 4 5 6 7", "0 1 2 3 inf 5 6 7", "finite"),
    ] {
        assert!(read(good.replace(from, to).as_bytes(), 0.).unwrap_err().to_string().contains(message), "{message}");
    }
}

#[test]
fn collections_preserve_times_paths_and_reject_unsupported_members() {
    let doc = r#"<VTKFile type="Collection"><Collection><DataSet timestep="0" file="first.vtu"/><DataSet timestep="0.125" part="0" file="time/last.vtu"/></Collection></VTKFile>"#;
    assert_eq!(collection(doc.as_bytes()).unwrap(), [(0., "first.vtu".into()), (0.125, "time/last.vtu".into())]);
    for (from, to) in [
        ("0.125", "NaN"),
        ("0.125", "0"),
        ("part=\"0\"", "part=\"1\""),
        ("first.vtu", "../outside.vtu"),
        ("first.vtu", "/outside.vtu"),
        ("first.vtu", r"C:\outside.vtu"),
        ("first.vtu", "recursive.pvd"),
    ] {
        assert!(collection(doc.replace(from, to).as_bytes()).is_err());
    }
}

#[test]
fn declared_tuple_counts_and_exact_integer_conversion() {
    let field = r#"<DataArray type="Int64" Name="labels" NumberOfTuples="4">-5 0 9007199254740992 12</DataArray>"#;
    let frame = read(&grid(POINTS, field, b"", ""), 0.).unwrap();
    assert_eq!(frame.point_data["labels"].values[2], 9_007_199_254_740_992.);
    for (from, to, message) in [
        ("Tuples=\"4\"", "Tuples=\"3\"", "NumberOfTuples"),
        ("9007199254740992", "9007199254740993", "represented exactly"),
        ("9007199254740992", "9223372036854775807", "represented exactly"),
    ] {
        assert!(read(&grid(POINTS, &field.replace(from, to), b"", ""), 0.).unwrap_err().to_string().contains(message));
    }
    let unsigned = field.replace("Int64", "UInt64").replace("-5", "18446744073709551615");
    assert!(read(&grid(POINTS, &unsigned, b"", ""), 0.).unwrap_err().to_string().contains("represented exactly"));
}

#[test]
fn a_collection_frame_respects_its_remaining_storage() {
    let bytes = grid(POINTS, POINT_FIELD, b"", "");
    let charge = read(&bytes, 0.0).unwrap().storage_bytes();
    assert_eq!(super::read_with_budget(&bytes, 0.0, charge).unwrap().storage_bytes(), charge);
    assert!(matches!(super::read_with_budget(&bytes, 0.0, charge - 1), Err(crate::Error::Limit(_))));
}

#[test]
fn a_pvtu_indexes_exactly_one_relative_serial_piece() {
    let doc = r#"<VTKFile type="PUnstructuredGrid"><PUnstructuredGrid GhostLevel="0"><PPoints><PDataArray type="Float64" NumberOfComponents="3"/></PPoints><Piece Source="step/grid.VTU"/></PUnstructuredGrid></VTKFile>"#;
    assert_eq!(single_piece(doc.as_bytes()).unwrap(), "step/grid.VTU");
    for name in ["../escape.vtu", "/outside.vtu", r"C:\temp\x.vtu", "nested.pvtu", "index.pvd"] {
        assert!(single_piece(doc.replace("step/grid.VTU", name).as_bytes()).is_err());
    }
    let extra = doc.replace("</PUnstructuredGrid>", r#"<Piece Source="part1.vtu"/></PUnstructuredGrid>"#);
    assert!(single_piece(extra.as_bytes()).unwrap_err().to_string().contains("multi-piece"));
    assert!(single_piece(doc.replace(r#"<Piece Source="step/grid.VTU"/>"#, "").as_bytes()).is_err());
}
