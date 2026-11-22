fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).ok_or("missing output path")?;
    std::fs::write(path, fieldcraft_geometry::sample::step_assembly(10.0)?)?;
    Ok(())
}
