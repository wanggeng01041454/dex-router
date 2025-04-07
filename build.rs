pub fn main() {
  println!("cargo:rerun-if-changed=proto/router.proto");
  tonic_build::compile_protos("proto/router.proto").unwrap();
}
