use crate::lib::environment::Environment;
use crate::lib::error::{DfxError, DfxResult};
use ic_agent::Blob;
use serde_idl::{Encode, IDLArgs, IDLProg, EMPTY_DIDL};

pub mod assets;
pub mod clap;

/// Deserialize and print return values from canister method.
pub fn print_idl_blob(blob: &Blob) -> Result<(), serde_idl::Error> {
    let result = serde_idl::IDLArgs::from_bytes(&(*blob.0));
    if result.is_err() {
        let hex_string = hex::encode(&(*blob.0));
        eprintln!("Error deserializing blob 0x{}", hex_string);
    }
    println!("{}", result?);
    Ok(())
}

/// Parse IDL file into AST. This is a best effort function: it will succeed if
/// the IDL file can be type checked by didc, parsed in Rust parser, and has an
/// actor in the IDL file. If anything fails, it returns None.
pub fn load_idl_file(env: &dyn Environment, idl_path: &std::path::Path) -> Option<IDLProg> {
    let mut didc = env.get_cache().get_binary_command("didc").ok()?;
    let status = didc.arg("--check").arg(&idl_path).status().ok()?;
    if !status.success() {
        return None;
    }
    let idl_file = std::fs::read_to_string(idl_path).ok()?;
    let ast = idl_file.parse::<IDLProg>().ok()?;
    if ast.actor.is_some() {
        Some(ast)
    } else {
        None
    }
}

pub fn blob_from_arguments(arguments: Option<&str>, arg_type: Option<&str>) -> DfxResult<Blob> {
    let arg_type = arg_type.unwrap_or("idl");

    if let Some(a) = arguments {
        match arg_type {
            "string" => Ok(Encode!(&a)),
            "number" => Ok(Encode!(&a.parse::<u64>().map_err(|e| {
                DfxError::InvalidArgument(format!(
                    "Argument is not a valid 64-bit unsigned integer: {}",
                    e
                ))
            })?)),
            "raw" => Ok(hex::decode(&a).map_err(|e| {
                DfxError::InvalidArgument(format!("Argument is not a valid hex string: {}", e))
            })?),
            "idl" => {
                let args: IDLArgs = a
                    .parse()
                    .map_err(|e| DfxError::InvalidArgument(format!("Invalid IDL: {}", e)))?;
                Ok(args.to_bytes().map_err(|e| {
                    DfxError::InvalidData(format!("Unable to convert IDL to bytes: {}", e))
                })?)
            }
            v => Err(DfxError::Unknown(format!("Invalid type: {}", v))),
        }
        .map(Blob::from)
    } else {
        match arg_type {
            "raw" => Ok(Blob::empty()),
            _ => Ok(Blob::from(EMPTY_DIDL)),
        }
    }
}
