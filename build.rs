fn main() {
  let proto_files = [
    "rpc/proto/base/v1/model.proto",
    "rpc/proto/query/v1/model.proto",
    "rpc/proto/query/v1/service.proto",
    "rpc/proto/router/v1/model.proto",
    "rpc/proto/router/v1/service.proto",
  ];

  let include_paths = ["."];
  tonic_build::configure().compile_protos(&proto_files, &include_paths).unwrap_or_else(|e| panic!("Failed to compile protos {:?}", e));
}
