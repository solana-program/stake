#![cfg(feature = "codama")]

use {codama::Codama, serde_json::Value, std::path::Path};

#[test]
fn test_all_codama_strings_use_u64_size_prefix() {
    let idl = load_idl();

    json_value_iter(&idl)
        .filter(|value| {
            value["kind"] == "sizePrefixTypeNode" && value["type"]["kind"] == "stringTypeNode"
        })
        .for_each(|value| {
            assert_eq!(value["prefix"]["kind"], "numberTypeNode");
            assert_eq!(value["prefix"]["endian"], "le");
            assert_eq!(value["prefix"]["format"], "u64");
        });
}

#[test]
fn test_all_codama_enums_use_u32_discriminators() {
    let idl = load_idl();

    json_value_iter(&idl)
        .filter(|value| value["kind"] == "enumTypeNode")
        .for_each(|value| {
            assert_eq!(value["size"]["kind"], "numberTypeNode");
            assert_eq!(value["size"]["endian"], "le");
            assert_eq!(value["size"]["format"], "u32");
        });
}

fn load_idl() -> Value {
    let crate_path = Path::new(env!("CARGO_MANIFEST_DIR"));
    let idl_json = Codama::load(crate_path).unwrap().get_json_idl().unwrap();
    serde_json::from_str(&idl_json).unwrap()
}

fn json_value_iter(root: &Value) -> impl Iterator<Item = &Value> {
    let mut stack = vec![root];

    std::iter::from_fn(move || {
        let value = stack.pop()?;

        match value {
            Value::Array(values) => stack.extend(values.iter()),
            Value::Object(values) => stack.extend(values.values()),
            _ => {}
        }

        Some(value)
    })
}
