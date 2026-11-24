fn main() -> Result<(), slint_build::CompileError> {
    slint_build::compile_with_config(
        "ui/lib.slint",
        slint_build::CompilerConfiguration::new().with_style("fluent".into()),
    )?;
    Ok(())
}
