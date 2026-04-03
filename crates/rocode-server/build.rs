use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;

fn find_npm() -> Option<&'static str> {
    let candidates: &[&str] = if cfg!(windows) {
        &["npm.cmd", "npm"]
    } else {
        &["npm", "npm.cmd"]
    };

    candidates.iter().copied().find(|candidate| {
        Command::new(candidate)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    })
}

fn main() {
    build_web_dir("web");
    generate_web_assets();
}

fn build_web_dir(dir_name: &str) {
    let web_dir = Path::new(dir_name);
    let src_dir = web_dir.join("src");
    let dist_dir = web_dir.join("dist");
    let node_modules = web_dir.join("node_modules");
    let package_json = web_dir.join("package.json");
    let package_lock = web_dir.join("package-lock.json");
    let tsconfig = web_dir.join("tsconfig.json");
    let vite_config = web_dir.join("vite.config.ts");
    let index_html = web_dir.join("index.html");
    let cache_dir = web_dir.join(".build-cache");
    let install_stamp = cache_dir.join("npm-install.stamp");
    let build_stamp = cache_dir.join("web-build.stamp");

    println!("cargo:rerun-if-changed={dir_name}/src");
    println!("cargo:rerun-if-changed={dir_name}/index.html");
    println!("cargo:rerun-if-changed={dir_name}/vite.config.ts");
    println!("cargo:rerun-if-changed={dir_name}/package.json");
    println!("cargo:rerun-if-changed={dir_name}/package-lock.json");
    println!("cargo:rerun-if-changed={dir_name}/tsconfig.json");

    if !src_dir.exists() {
        panic!("{dir_name}/src directory not found");
    }

    let install_fingerprint = fingerprint_inputs(&[package_json.as_path(), package_lock.as_path()]);
    let build_fingerprint = fingerprint_inputs(&[
        package_json.as_path(),
        package_lock.as_path(),
        tsconfig.as_path(),
        vite_config.as_path(),
        index_html.as_path(),
        src_dir.as_path(),
    ]);
    let dist_ready = has_prebuilt_dist(&dist_dir);

    let Some(npm) = find_npm() else {
        if dist_ready {
            println!("cargo:warning={dir_name}: using pre-built dist/ (npm executable not found)");
            return;
        }
        panic!("failed to find npm executable (`npm` or `npm.cmd`) for {dir_name} build");
    };

    let install_fingerprint_changed =
        read_stamp(&install_stamp).as_deref() != Some(install_fingerprint.as_str());
    let build_fingerprint_changed =
        read_stamp(&build_stamp).as_deref() != Some(build_fingerprint.as_str());

    if dist_ready && !build_fingerprint_changed {
        println!("cargo:warning={dir_name}: web inputs unchanged; skipping npm build");
        return;
    }

    if install_fingerprint_changed || !node_modules.exists() {
        let install_status = Command::new(npm)
            .arg("install")
            .current_dir(web_dir)
            .status()
            .unwrap_or_else(|_| panic!("failed to run `npm install` in {dir_name}/"));

        if !install_status.success() {
            panic!(
                "{dir_name} npm install failed with status: {}",
                install_status
            );
        }

        write_stamp(&install_stamp, &install_fingerprint).unwrap_or_else(|error| {
            panic!("failed to persist install fingerprint for {dir_name}: {error}")
        });
    } else {
        println!("cargo:warning={dir_name}: npm dependencies unchanged; skipping npm install");
    }

    let status = Command::new(npm)
        .arg("run")
        .arg("build")
        .current_dir(web_dir)
        .status()
        .unwrap_or_else(|_| panic!("failed to run `npm run build` in {dir_name}/"));

    if !status.success() {
        panic!("{dir_name} build failed with status: {}", status);
    }

    write_stamp(&build_stamp, &build_fingerprint).unwrap_or_else(|error| {
        panic!("failed to persist build fingerprint for {dir_name}: {error}")
    });
}

fn has_prebuilt_dist(dist_dir: &Path) -> bool {
    dist_dir.exists()
        && dist_dir.join("index.html").exists()
        && dist_dir.join("app.js").exists()
        && dist_dir.join("app.css").exists()
}

fn generate_web_assets() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR missing"));
    let assets_dir = manifest_dir.join("web").join("dist").join("assets");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR missing"));
    let out_file = out_dir.join("web_assets.rs");

    let mut entries = Vec::new();
    if assets_dir.exists() {
        collect_asset_files(&assets_dir, &assets_dir, &mut entries);
        entries.sort_by(|left, right| left.0.cmp(&right.0));
    }

    let mut generated = String::from(
        "pub(crate) fn web_asset_bytes(path: &str) -> Option<(&'static [u8], &'static str)> {\n    match path {\n",
    );

    for (relative, absolute) in entries {
        let mime = mime_for_path(&relative);
        generated.push_str(&format!(
            "        {:?} => Some((include_bytes!(r#\"{}\"#), {:?})),\n",
            relative,
            absolute.display(),
            mime
        ));
    }

    generated.push_str("        _ => None,\n    }\n}\n");
    fs::write(out_file, generated).expect("failed to write generated next web asset map");
}

fn fingerprint_inputs(paths: &[&Path]) -> String {
    let mut hasher = StableHasher::default();
    for path in paths {
        hash_path(*path, &mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn hash_path(path: &Path, hasher: &mut StableHasher) {
    if !path.exists() {
        hasher.write_bytes(b"missing");
        hasher.write_bytes(path.to_string_lossy().as_bytes());
        return;
    }

    if path.is_dir() {
        hasher.write_bytes(b"dir");
        hasher.write_bytes(path.to_string_lossy().as_bytes());
        let mut entries = match fs::read_dir(path) {
            Ok(entries) => entries
                .flatten()
                .map(|entry| entry.path())
                .collect::<Vec<_>>(),
            Err(_) => {
                hasher.write_bytes(b"read-dir-error");
                return;
            }
        };
        entries.sort();
        for entry in entries {
            hash_path(&entry, hasher);
        }
        return;
    }

    hasher.write_bytes(b"file");
    hasher.write_bytes(path.to_string_lossy().as_bytes());
    match fs::read(path) {
        Ok(contents) => hasher.write_bytes(&contents),
        Err(_) => hasher.write_bytes(b"read-file-error"),
    }
}

fn read_stamp(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
}

fn write_stamp(path: &Path, value: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, value)
}

#[derive(Default)]
struct StableHasher {
    state: u64,
}

impl StableHasher {
    const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;

    fn write_bytes(&mut self, bytes: &[u8]) {
        if self.state == 0 {
            self.state = Self::OFFSET_BASIS;
        }
        for byte in bytes {
            self.state ^= u64::from(*byte);
            self.state = self.state.wrapping_mul(Self::PRIME);
        }
        self.state ^= 0xff;
        self.state = self.state.wrapping_mul(Self::PRIME);
    }

    fn finish(&self) -> u64 {
        if self.state == 0 {
            Self::OFFSET_BASIS
        } else {
            self.state
        }
    }
}

fn collect_asset_files(base: &Path, current: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_asset_files(base, &path, out);
            continue;
        }

        let Ok(relative) = path.strip_prefix(base) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        out.push((relative, path));
    }
}

fn mime_for_path(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|ext| ext.to_str()) {
        Some("js") | Some("mjs") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        _ => "application/octet-stream",
    }
}
