use embed_manifest::manifest::ExecutionLevel;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_manifest::embed_manifest(
            embed_manifest::new_manifest("monsgeek-unbrick")
                .requested_execution_level(ExecutionLevel::RequireAdministrator),
        )
        .expect("failed to embed manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
