//! DWG output: a DXF drawing written by [`crate::write_dxf_drawing`], converted to an AutoCAD 2004
//! (AC1018) DWG with the acadrust crate (MPL-2.0, used unmodified). The reader and writer run
//! under `catch_unwind`, so a failure in the third-party code is an error, never a crash.

use std::io::Cursor;
use std::panic::AssertUnwindSafe;

/// Largest DXF converted (the drawing sheets SolidCraft writes are far smaller).
const MAX_DXF_BYTES: usize = 256 << 20;

/// Convert DXF text into DWG bytes (AutoCAD 2004 or the DXF's own newer version).
#[cfg(not(target_arch = "wasm32"))]
pub fn dxf_to_dwg(dxf: &[u8]) -> Result<Vec<u8>, String> {
    if dxf.len() > MAX_DXF_BYTES {
        return Err("the drawing is too large to save as DWG".into());
    }
    let data = dxf.to_vec();
    let mut doc = std::panic::catch_unwind(move || acadrust::DxfReader::from_reader(Cursor::new(data)).and_then(|r| r.read()))
        .map_err(|_| "the DXF to DWG conversion failed".to_string())?
        .map_err(|e| format!("DXF: {e}"))?;
    if doc.version < acadrust::DxfVersion::AC1018 {
        doc.version = acadrust::DxfVersion::AC1018;
    }
    std::panic::catch_unwind(AssertUnwindSafe(|| {
        add_variable_dictionary(&mut doc);
        current_layer(&mut doc);
    }))
    .map_err(|_| "the DXF to DWG conversion failed on this drawing".to_string())?;
    std::panic::catch_unwind(AssertUnwindSafe(|| acadrust::DwgWriter::write_to_vec(&doc)))
        .map_err(|_| "the DWG writer failed on this drawing".to_string())?
        .map_err(|e| format!("DWG write: {e}"))
}

/// The browser build has no DWG writer.
#[cfg(target_arch = "wasm32")]
pub fn dxf_to_dwg(_dxf: &[u8]) -> Result<Vec<u8>, String> {
    Err("DWG output is not available in the browser; save the drawing as DXF".into())
}

/// Whether bytes start like a DWG file ("AC10..").
pub fn is_dwg(bytes: &[u8]) -> bool {
    bytes.len() >= 6 && bytes.starts_with(b"AC10")
}

/// The current layer by its name: the handle read with the DXF needn't be the layer's in the
/// document, in which case the drawing would reopen with layer "0" current.
#[cfg(not(target_arch = "wasm32"))]
fn current_layer(doc: &mut acadrust::CadDocument) {
    let name = doc.header.current_layer_name.clone();
    if let Some(layer) = doc.layers.get(&name) {
        doc.header.current_layer_handle = layer.handle;
    }
}

/// The drawing variables dictionary, which some readers of 2004 DWGs need to read the drawing at
/// all; added with default values when the drawing has none.
#[cfg(not(target_arch = "wasm32"))]
fn add_variable_dictionary(doc: &mut acadrust::CadDocument) {
    use acadrust::objects::{Dictionary, DictionaryVariable, ObjectType};
    const NAME: &str = "AcDbVariableDictionary";
    if doc.version >= acadrust::DxfVersion::AC1032 {
        return;
    }
    let root_handle = doc.header.named_objects_dict_handle;
    match doc.objects.get(&root_handle) {
        Some(ObjectType::Dictionary(root)) if root.get(NAME).is_none() => {}
        _ => return,
    }
    let variables = [
        ("CMLEADERSTYLE", "Standard"),
        ("CTABLESTYLE", "Standard"),
        ("CANNOSCALE", "1:1"),
        ("DIMASSOC", "2"),
        ("HIDETEXT", "1"),
        ("LAYEREVAL", "0"),
        ("LAYERNOTIFY", "0"),
    ];
    let dictionary_handle = doc.allocate_handle();
    let mut dictionary = Dictionary::new();
    dictionary.handle = dictionary_handle;
    dictionary.owner = root_handle;
    for (name, value) in variables {
        let handle = doc.allocate_handle();
        let mut variable = DictionaryVariable::new(name, value);
        variable.handle = handle;
        variable.owner_handle = dictionary_handle;
        dictionary.add_entry(name, handle);
        doc.objects.insert(handle, ObjectType::DictionaryVariable(variable));
    }
    doc.objects.insert(dictionary_handle, ObjectType::Dictionary(dictionary));
    if let Some(ObjectType::Dictionary(root)) = doc.objects.get_mut(&root_handle) {
        root.add_entry(NAME, dictionary_handle);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::{DxfEntity, DxfLayer, DxfShape, write_dxf_drawing};
    use solidcore_record::Vec2;

    #[test]
    fn a_drawing_converts_to_dwg() {
        let ents = vec![
            DxfEntity { layer: "VISIBLE".into(), shape: DxfShape::Line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)) },
            DxfEntity { layer: "VISIBLE".into(), shape: DxfShape::Circle { c: Vec2::new(5.0, 5.0), r: 2.0 } },
            DxfEntity { layer: "TEXT".into(), shape: DxfShape::Text { at: Vec2::new(0.0, -5.0), height: 3.5, angle_deg: 0.0, text: "FRONT".into() } },
        ];
        let dxf = write_dxf_drawing(&[DxfLayer::new("VISIBLE", 7), DxfLayer::new("TEXT", 7)], &ents).unwrap();
        let dwg = dxf_to_dwg(dxf.as_bytes()).unwrap();
        assert!(is_dwg(&dwg), "{:?}", &dwg[..dwg.len().min(8)]);
        assert!(dxf_to_dwg(b"garbage").is_err());
    }
}
