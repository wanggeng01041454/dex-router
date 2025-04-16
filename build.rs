use prost_build::Config;
use std::path::PathBuf;

fn main() {
  let mut config = Config::new();

  // 给所有 message 加 Serialize + Deserialize
  config.type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]");
  config.type_attribute(".", "#[serde(rename_all = \"camelCase\")]");

  let proto_root = "rpc/proto";

  let proto_files = [
    format!("{}/base/v1/model.proto", proto_root),
    format!("{}/query/v1/model.proto", proto_root),
    format!("{}/query/v1/service.proto", proto_root),
    format!("{}/router/v1/model.proto", proto_root),
    format!("{}/router/v1/service.proto", proto_root),
  ];
  //let out_dir = PathBuf::from("src/service/pb");

  tonic_build::configure()
    .build_client(true)
    .build_server(true)
    .out_dir("src/service/pb") // 输出到 src/pb
    .compile_protos_with_config(config, &proto_files.iter().map(|s| s.as_str()).collect::<Vec<_>>(), &[proto_root])
    .unwrap_or_else(|e| panic!("Failed to compile protos: {:?}", e));
}
