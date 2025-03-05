use std::fs;
use std::path::PathBuf;

fn main() {
    let proto_dir = PathBuf::from("protos/");

    // Collect all .protos files in the specified directory.
    // Note: Using .protos extension as seen in the pod project instead of .proto
    let protos: Vec<PathBuf> = fs::read_dir(&proto_dir)
        .expect("Failed to read proto directory")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("protos") {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    // Print the files found for debugging
    for proto in &protos {
        println!("cargo:warning=Found proto file: {}", proto.display());
    }

    // Configure tonic-build with explicit type attributes for each message
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        // Add derive attributes for all messages by default
        .type_attribute(".", "#[derive(std::hash::Hash, std::cmp::Eq)]")
        // Use fully qualified paths for serde macros
        .type_attribute("replicaapi.Transaction", "#[derive(::serde::Serialize, ::serde::Deserialize)]")
        .type_attribute("replicaapi.VoteRecord", "#[derive(::serde::Serialize, ::serde::Deserialize)]")
        .compile_protos(&protos, &[proto_dir])
        .unwrap_or_else(|e| panic!("Failed to compile protos: {}", e));

    // Instruct Cargo to re-run the build script if any proto file changes.
    for proto in protos {
        println!("cargo:rerun-if-changed={}", proto.display());
    }
}
