use std::fmt::{Show,Formatter};
use std::fmt;
use std::hash::Hasher;
use std::hash::sip::SipHasher;
use std::str;

use core::source::{Source, SourceId, GitKind, Location, Remote, Local};
use core::{Package,PackageId,Summary};
use util::{CargoResult, Config, to_hex};
use sources::PathSource;
use sources::git::utils::{GitReference,GitRemote,Master,Other};

/* TODO: Refactor GitSource to delegate to a PathSource
 */
pub struct GitSource<'a, 'b> {
    remote: GitRemote,
    reference: GitReference,
    db_path: Path,
    checkout_path: Path,
    path_source: PathSource,
    config: &'a mut Config<'b>
}

impl<'a, 'b> GitSource<'a, 'b> {
    pub fn new<'a, 'b>(source_id: &SourceId, config: &'a mut Config<'b>) -> GitSource<'a, 'b> {
        assert!(source_id.is_git(), "id is not git, id={}", source_id);

        let reference = match source_id.kind {
            GitKind(ref reference) => reference,
            _ => fail!("Not a git source; id={}", source_id)
        };

        let remote = GitRemote::new(source_id.get_location());
        let ident = ident(source_id.get_location());

        let db_path = config.git_db_path()
            .join(ident.as_slice());

        let checkout_path = config.git_checkout_path()
            .join(ident.as_slice()).join(reference.as_slice());

        let path_source = PathSource::new(&checkout_path, source_id);

        GitSource {
            remote: remote,
            reference: GitReference::for_str(reference.as_slice()),
            db_path: db_path,
            checkout_path: checkout_path,
            path_source: path_source,
            config: config
        }
    }

    pub fn get_namespace(&self) -> &Location {
        self.remote.get_location()
    }
}

fn ident(location: &Location) -> String {
    let hasher = SipHasher::new_with_keys(0,0);

    // FIXME: this really should be able to not use to_str() everywhere, but the
    //        compiler seems to currently ask for static lifetimes spuriously.
    //        Perhaps related to rust-lang/rust#15144
    let ident = match *location {
        Local(ref path) => {
            let last = path.components().last().unwrap();
            str::from_utf8(last).unwrap().to_string()
        }
        Remote(ref url) => {
            let path = canonicalize_url(url.path.path.as_slice());
            path.as_slice().split('/').last().unwrap().to_string()
        }
    };

    let ident = if ident.as_slice() == "" {
        "_empty".to_string()
    } else {
        ident
    };

    let location = canonicalize_url(location.to_string().as_slice());

    format!("{}-{}", ident, to_hex(hasher.hash(&location.as_slice())))
}

fn strip_trailing_slash(path: &str) -> &str {
    // Remove the trailing '/' so that 'split' doesn't give us
    // an empty string, making '../foo/' and '../foo' both
    // result in the name 'foo' (#84)
    if path.as_bytes().last() != Some(&('/' as u8)) {
        path.clone()
    } else {
        path.slice(0, path.len() - 1)
    }
}

// Some hacks and heuristics for making equivalent URLs hash the same
pub fn canonicalize_url(url: &str) -> String {
    let url = strip_trailing_slash(url);

    // HACKHACK: For github URL's specifically just lowercase
    // everything.  GitHub traits both the same, but they hash
    // differently, and we're gonna be hashing them. This wants a more
    // general solution, and also we're almost certainly not using the
    // same case conversion rules that GitHub does. (#84)

    let lower_url = url.chars().map(|c|c.to_lowercase()).collect::<String>();
    let url = if lower_url.as_slice().contains("github.com") {
        if lower_url.as_slice().starts_with("https") {
            lower_url
        } else {
            let pos = lower_url.as_slice().find_str("://").unwrap_or(0);
            "https".to_string() + lower_url.as_slice().slice_from(pos)
        }
    } else {
        url.to_string()
    };

    // Repos generally can be accessed with or w/o '.git'
    let url = if !url.as_slice().ends_with(".git") {
        url
    } else {
        url.as_slice().slice(0, url.len() - 4).to_string()
    };

    return url;
}

impl<'a, 'b> Show for GitSource<'a, 'b> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        try!(write!(f, "git repo at {}", self.remote.get_location()));

        match self.reference {
            Master => Ok(()),
            Other(ref reference) => write!(f, " ({})", reference)
        }
    }
}

impl<'a, 'b> Source for GitSource<'a, 'b> {
    fn update(&mut self) -> CargoResult<()> {
        let should_update = self.config.update_remotes() || {
            !self.remote.has_ref(&self.db_path, self.reference.as_slice()).is_ok()
        };

        let repo = if should_update {
            try!(self.config.shell().status("Updating",
                format!("git repository `{}`", self.remote.get_location())));

            log!(5, "updating git source `{}`", self.remote);
            try!(self.remote.checkout(&self.db_path))
        } else {
            self.remote.db_at(&self.db_path)
        };

        try!(repo.copy_to(self.reference.as_slice(), &self.checkout_path));

        self.path_source.update()
    }

    fn list(&self) -> CargoResult<Vec<Summary>> {
        self.path_source.list()
    }

    fn download(&self, _: &[PackageId]) -> CargoResult<()> {
        // TODO: assert! that the PackageId is contained by the source
        Ok(())
    }

    fn get(&self, ids: &[PackageId]) -> CargoResult<Vec<Package>> {
        log!(5, "getting packages for package ids `{}` from `{}`", ids, self.remote);
        self.path_source.get(ids)
    }

    fn fingerprint(&self, _pkg: &Package) -> CargoResult<String> {
        let db = self.remote.db_at(&self.db_path);
        db.rev_for(self.reference.as_slice())
    }
}

#[cfg(test)]
mod test {
    use url::Url;
    use core::source::Remote;
    use super::ident;

    #[test]
    pub fn test_url_to_path_ident_with_path() {
        let ident = ident(&Remote(url("https://github.com/carlhuda/cargo")));
        assert_eq!(ident.as_slice(), "cargo-0eed735c8ffd7c88");
    }

    #[test]
    pub fn test_url_to_path_ident_without_path() {
        let ident = ident(&Remote(url("https://github.com")));
        assert_eq!(ident.as_slice(), "_empty-fc065c9b6b16fc00");
    }

    #[test]
    fn test_canonicalize_idents_by_stripping_trailing_url_slash() {
        let ident1 = ident(&Remote(url("https://github.com/PistonDevelopers/piston/")));
        let ident2 = ident(&Remote(url("https://github.com/PistonDevelopers/piston")));
        assert_eq!(ident1, ident2);
    }

    #[test]
    fn test_canonicalize_idents_by_lowercasing_github_urls() {
        let ident1 = ident(&Remote(url("https://github.com/PistonDevelopers/piston")));
        let ident2 = ident(&Remote(url("https://github.com/pistondevelopers/piston")));
        assert_eq!(ident1, ident2);
    }

    #[test]
    fn test_canonicalize_idents_by_stripping_dot_git() {
        let ident1 = ident(&Remote(url("https://github.com/PistonDevelopers/piston")));
        let ident2 = ident(&Remote(url("https://github.com/PistonDevelopers/piston.git")));
        assert_eq!(ident1, ident2);
    }

    #[test]
    fn test_canonicalize_idents_different_protocls() {
        let ident1 = ident(&Remote(url("https://github.com/PistonDevelopers/piston")));
        let ident2 = ident(&Remote(url("git://github.com/PistonDevelopers/piston")));
        assert_eq!(ident1, ident2);
    }

    fn url(s: &str) -> Url {
        from_str(s).unwrap()
    }
}
