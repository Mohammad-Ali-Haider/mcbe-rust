//! Discover the installed DXC compiler without copying SDK/runtime binaries into source.
use std::path::{Path, PathBuf};
pub struct Selection {
    pub compiler: wgpu::Dx12Compiler,
    pub name: &'static str,
    pub directory: Option<PathBuf>,
}
fn pair(path: &Path) -> bool {
    path.join("dxcompiler.dll").is_file() && path.join("dxil.dll").is_file()
}
pub fn select() -> Selection {
    if let Some(compiler) = wgpu::Dx12Compiler::from_env() {
        return Selection {
            compiler,
            name: "environment",
            directory: None,
        };
    }
    let mut choices = Vec::new();
    if let Some(dir) = std::env::var_os("MCBE_DXC_DIR") {
        choices.push(PathBuf::from(dir));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        choices.push(dir.to_path_buf());
    }
    if let Some(root) = std::env::var_os("ProgramFiles(x86)") {
        let bin = PathBuf::from(root).join("Windows Kits/10/bin");
        let mut versions = std::fs::read_dir(&bin)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect::<Vec<_>>();
        versions.sort_by_key(|p| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .split('.')
                .map(|v| v.parse::<u32>().unwrap_or(0))
                .collect::<Vec<_>>()
        });
        for version in versions.into_iter().rev() {
            choices.push(version.join(if cfg!(target_arch = "aarch64") {
                "arm64"
            } else {
                "x64"
            }));
        }
    }
    if let Some(dir) = choices.into_iter().find(|p| pair(p)) {
        return Selection {
            compiler: wgpu::Dx12Compiler::DynamicDxc {
                dxc_path: dir.join("dxcompiler.dll").to_string_lossy().into_owned(),
                dxil_path: dir.join("dxil.dll").to_string_lossy().into_owned(),
                max_shader_model: wgpu::DxcShaderModel::V6_5,
            },
            name: "dxc",
            directory: Some(dir),
        };
    }
    Selection {
        compiler: wgpu::Dx12Compiler::Fxc,
        name: "fxc",
        directory: None,
    }
}
