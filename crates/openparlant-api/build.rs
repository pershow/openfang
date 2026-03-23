use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=OPENFANG_SKIP_FRONTEND_BUILD");
    for path in [
        "../../frontend/package.json",
        "../../frontend/package-lock.json",
        "../../frontend/tsconfig.json",
        "../../frontend/vite.config.ts",
        "../../frontend/index.html",
        "../../frontend/public",
        "../../frontend/src",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }

    let skip = env::var("OPENFANG_SKIP_FRONTEND_BUILD")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if skip {
        return;
    }

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    let frontend_dir = manifest_dir.join("../../frontend");
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };

    let status = Command::new(npm)
        .args(["run", "build"])
        .current_dir(&frontend_dir)
        .status()
        .unwrap_or_else(|error| {
            panic!(
                "failed to launch frontend build with `{npm}` in {}: {error}. Install Node.js/npm, run `npm install` in frontend/, or set OPENFANG_SKIP_FRONTEND_BUILD=1 to use the existing dist.",
                frontend_dir.display()
            )
        });

    if !status.success() {
        panic!(
            "frontend build failed in {}. Run `npm install` in frontend/ and make sure `npm run build` passes.",
            frontend_dir.display()
        );
    }
}
