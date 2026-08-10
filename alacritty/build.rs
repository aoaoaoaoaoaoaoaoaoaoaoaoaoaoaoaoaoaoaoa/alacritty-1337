use std::env;
use std::error::Error;
use std::fs::File;
use std::path::Path;

#[cfg(windows)]
use std::fs;

use gl_generator::{Api, Fallbacks, GlobalGenerator, Profile, Registry};

fn main() -> Result<(), Box<dyn Error>> {
    let version = env!("CARGO_PKG_VERSION");
    println!("cargo:rustc-env=VERSION={version}");

    let dest = env::var("OUT_DIR")?;
    let mut file = File::create(Path::new(&dest).join("gl_bindings.rs"))?;

    Registry::new(
        Api::Gl,
        (3, 3),
        Profile::Core,
        Fallbacks::All,
        ["GL_ARB_blend_func_extended", "GL_KHR_robustness", "GL_KHR_debug"],
    )
    .write_bindings(GlobalGenerator, &mut file)?;

    #[cfg(windows)]
    compile_windows_resources(version, Path::new(&dest))?;

    Ok(())
}

#[cfg(windows)]
fn compile_windows_resources(version: &str, out_dir: &Path) -> Result<(), Box<dyn Error>> {
    let mut components = version.split('.');
    let major: u16 = components.next().ok_or("missing major version")?.parse()?;
    let minor: u16 = components.next().ok_or("missing minor version")?.parse()?;
    let patch: u16 = components.next().ok_or("missing patch version")?.parse()?;
    if components.next().is_some() {
        return Err("Windows resources require a three-component version".into());
    }

    let version_header = format!(
        "#define VERSION_MAJOR {major}\n\
         #define VERSION_MINOR {minor}\n\
         #define VERSION_PATCH {patch}\n\
         #define VERSION_STRING \"{version}\\0\"\n"
    );
    fs::write(out_dir.join("version.rc"), version_header)?;
    embed_resource::compile("./windows/alacritty.rc", embed_resource::NONE).manifest_required()?;

    Ok(())
}
