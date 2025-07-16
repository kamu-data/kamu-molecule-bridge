use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    // Preparing the build information
    vergen::EmitBuilder::builder()
        .all_build()
        .all_git()
        .all_rustc()
        .all_cargo()
        .fail_on_error()
        .emit()?;

    Ok(())
}
