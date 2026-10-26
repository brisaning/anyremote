fn main() {
    println!("cargo:rerun-if-changed=proto/arsp.proto");
    let file = protox::compile(["proto/arsp.proto"], ["proto"]).expect("compilar proto con protox");
    prost_build::Config::new()
        .compile_fds(file)
        .expect("generar tipos Rust con prost");
}
