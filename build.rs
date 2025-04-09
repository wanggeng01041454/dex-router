fn main() {
  let proto_files = [
    "rpc/proto/base/v1/model.proto",
    "rpc/proto/query/v1/service.proto",
    "rpc/proto/query/v1/model.proto",
    "rpc/proto/router/v1/service.proto",
    "rpc/proto/router/v1/model.proto",
  ];

  for proto_file in &proto_files {
    println!("cargo:rerun-if-changed={}", proto_file);
    tonic_build::compile_protos(&[proto_file]).unwrap_or_else(|e| panic!("Failed to compile protos {:?}", e));
  }
}
