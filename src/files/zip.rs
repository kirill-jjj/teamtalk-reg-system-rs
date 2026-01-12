use anyhow::Result;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use walkdir::WalkDir;
use zip::write::FileOptions;

pub fn create_client_zip(
    template_path: &str,
    output_path: &Path,
    tt_filename: &str,
    tt_content: &str,
) -> Result<()> {
    let tpl_path = Path::new(template_path);

    if !tpl_path.exists() {
        return Err(anyhow::anyhow!(
            "Template directory does not exist: {}",
            template_path
        ));
    }

    let file = File::create(output_path)?;
    let mut zip = zip::ZipWriter::new(file);

    let options = FileOptions::<()>::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    let walk = WalkDir::new(tpl_path);
    for entry in walk {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            continue;
        }

        let name = path
            .strip_prefix(tpl_path)?
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid path encoding"))?;

        let zip_entry_name = name.replace('\\', "/");

        zip.start_file(zip_entry_name, options)?;
        let mut f = File::open(path)?;
        std::io::copy(&mut f, &mut zip)?;
    }

    let tt_entry_name = format!("Client/{}", tt_filename);
    zip.start_file(tt_entry_name, options)?;
    zip.write_all(tt_content.as_bytes())?;

    zip.finish()?;
    Ok(())
}
