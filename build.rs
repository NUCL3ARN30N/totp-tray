fn main() {
    // "material" follows the Windows light/dark setting.
    let config = slint_build::CompilerConfiguration::new().with_style("material".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("failed to compile ui/app.slint");
}
