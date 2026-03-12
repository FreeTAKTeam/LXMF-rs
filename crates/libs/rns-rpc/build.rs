use std::env;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .expect("workspace root");
    let proto_root = workspace_root.join("api/proto");
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("resolve vendored protoc");

    println!("cargo:rerun-if-changed={}", proto_root.display());
    println!("cargo:rerun-if-env-changed=PROTOC");

    env::set_var("PROTOC", protoc);

    let protos = [
        proto_root.join("lxmf/common/v1/interfaces.proto"),
        proto_root.join("lxmf/common/v1/pagination.proto"),
        proto_root.join("lxmf/runtime/v1/runtime.proto"),
        proto_root.join("lxmf/delivery/v1/delivery.proto"),
        proto_root.join("lxmf/command/v1/command.proto"),
        proto_root.join("lxmf/admin/v1/interface_admin.proto"),
        proto_root.join("lxmf/topics/v1/topics.proto"),
        proto_root.join("lxmf/attachments/v1/attachments.proto"),
        proto_root.join("lxmf/events/v1/events.proto"),
        proto_root.join("lxmf/identity/v1/identity.proto"),
        proto_root.join("lxmf/markers/v1/markers.proto"),
        proto_root.join("lxmf/peers/v1/peers.proto"),
    ];
    let descriptor_path =
        PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("lxmf-reflection-descriptor.bin");

    tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        .build_transport(true)
        .file_descriptor_set_path(&descriptor_path)
        .compile_protos(&protos, &[proto_root])
        .expect("compile gRPC protos");
}
