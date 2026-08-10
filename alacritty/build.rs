use std::env;
use std::error::Error;
use std::fs::File;
use std::path::Path;

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
    embed_resource::compile("./windows/alacritty.rc", embed_resource::NONE).manifest_required()?;

    Ok(())
}
