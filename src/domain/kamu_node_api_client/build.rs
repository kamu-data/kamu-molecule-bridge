fn main() {
    // NOTE: Rebuild crate if GQL assets changed
    println!("cargo:rerun-if-changed=./gql/");
}
