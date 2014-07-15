use serialize::Encodable;
use toml::Encoder;
use core::registry::PackageRegistry;
use core::{MultiShell, Source, Resolve, resolver};
use sources::{PathSource};
use util::config::{Config};
use util::{CargoResult};

pub fn generate_lockfile(manifest_path: &Path, shell: &mut MultiShell, update: bool) -> CargoResult<()> {
    log!(4, "compile; manifest-path={}", manifest_path.display());

    let mut source = PathSource::for_path(&manifest_path.dir_path());
    try!(source.update());

    // TODO: Move this into PathSource
    let package = try!(source.get_root_package());
    debug!("loaded package; package={}", package);

    for key in package.get_manifest().get_unused_keys().iter() {
        try!(shell.warn(format!("unused manifest key: {}", key)));
    }

    let source_ids = package.get_source_ids();

    let resolve = {
        let mut config = try!(Config::new(shell, update, None, None));

        let mut registry =
            try!(PackageRegistry::new(source_ids, vec![], &mut config));

        try!(resolver::resolve(package.get_package_id(),
                               package.get_dependencies(),
                               &mut registry))
    };

    write_resolve(resolve);
    Ok(())
}

fn write_resolve(resolve: Resolve) {
    let mut e = Encoder::new();
    let toml = resolve.encode(&mut e).unwrap();
    println!("{}", toml);
}
