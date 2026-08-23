use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let mut prost = prost_build::Config::new();
    prost.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);
    tonic_prost_build::configure()
        .type_attribute(
            ".shrimpman.discovery.v1.ServiceInstance",
            "#[derive(serde::Serialize, serde::Deserialize)]",
        )
        .type_attribute(
            ".shrimpman.discovery.v1.PublishedServiceInstance",
            "#[derive(serde::Serialize, serde::Deserialize)]",
        )
        .compile_with_config(prost, &["proto/discovery.proto"], &["proto"])?;

    Ok(())
}
