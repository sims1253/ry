use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// A temporary, on-disk project consumed by every test driver.
///
/// Named fixtures are copied from `ry-testkit/testdata`; programmatic helpers
/// cover the same filesystem shapes for generated tests.
pub struct FixtureProject {
    temp: tempfile::TempDir,
}

impl FixtureProject {
    pub fn empty() -> io::Result<Self> {
        Ok(Self {
            temp: tempfile::tempdir()?,
        })
    }

    pub fn from_fixture(name: impl AsRef<Path>) -> io::Result<Self> {
        let name = checked_relative(name.as_ref())?;
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join(name);
        if !source.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("fixture directory does not exist: {}", source.display()),
            ));
        }
        let project = Self::empty()?;
        copy_tree(&source, project.root())?;
        Ok(project)
    }

    pub fn root(&self) -> &Path {
        self.temp.path()
    }

    pub fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.root().join(relative)
    }

    pub fn write_file(
        &self,
        relative: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
    ) -> io::Result<PathBuf> {
        let relative = checked_relative(relative.as_ref())?;
        let path = self.root().join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, contents)?;
        Ok(path)
    }

    pub fn write_ry_toml(&self, contents: impl AsRef<str>) -> io::Result<PathBuf> {
        self.write_file("ry.toml", contents.as_ref())
    }

    pub fn write_description(&self, contents: impl AsRef<str>) -> io::Result<PathBuf> {
        self.write_file("DESCRIPTION", contents.as_ref())
    }

    pub fn write_namespace(&self, contents: impl AsRef<str>) -> io::Result<PathBuf> {
        self.write_file("NAMESPACE", contents.as_ref())
    }

    pub fn write_typeshed(
        &self,
        relative: impl AsRef<Path>,
        contents: impl AsRef<str>,
    ) -> io::Result<PathBuf> {
        let relative = checked_relative(relative.as_ref())?;
        self.write_file(Path::new("typeshed").join(relative), contents.as_ref())
    }

    pub fn write_serialized_data(
        &self,
        relative: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
    ) -> io::Result<PathBuf> {
        let relative = checked_relative(relative.as_ref())?;
        self.write_file(Path::new("data").join(relative), contents)
    }

    /// Create and return a workspace root below the fixture project.
    pub fn workspace_root(&self, relative: impl AsRef<Path>) -> io::Result<PathBuf> {
        let relative = checked_relative(relative.as_ref())?;
        let path = self.root().join(relative);
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    pub fn files(&self) -> io::Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        collect_files(self.root(), &mut files)?;
        files.sort();
        Ok(files)
    }
}

fn checked_relative(path: &Path) -> io::Result<&Path> {
    if path.as_os_str().is_empty()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "fixture path must be a non-empty relative path: {}",
                path.display()
            ),
        ));
    }
    Ok(path)
}

fn copy_tree(source: &Path, destination: &Path) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&target)?;
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            collect_files(&entry.path(), files)?;
        } else {
            files.push(entry.path());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_materializes_all_owned_fixture_shapes() {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("R/main.R", "x <- 1L\n").unwrap();
        fixture.write_ry_toml("ignore = [\"RY001\"]\n").unwrap();
        fixture.write_description("Package: fixture\n").unwrap();
        fixture.write_namespace("export(x)\n").unwrap();
        fixture.write_typeshed("fixture.json", "{}").unwrap();
        fixture
            .write_serialized_data("ordinary.rda", [1, 2, 3])
            .unwrap();
        fixture.workspace_root("roots/second").unwrap();

        assert!(fixture.path("R/main.R").is_file());
        assert!(fixture.path("typeshed/fixture.json").is_file());
        assert!(fixture.path("data/ordinary.rda").is_file());
        assert!(fixture.path("roots/second").is_dir());
    }

    #[test]
    fn named_fixture_is_copied_and_isolated() {
        let first = FixtureProject::from_fixture("shared").unwrap();
        let second = FixtureProject::from_fixture("shared").unwrap();
        first.write_file("R/diagnostic.R", "changed\n").unwrap();
        assert_ne!(
            fs::read(first.path("R/diagnostic.R")).unwrap(),
            fs::read(second.path("R/diagnostic.R")).unwrap()
        );
    }

    #[test]
    fn seed_fixtures_cover_the_shared_test_matrix() {
        let filtering = FixtureProject::from_fixture("filtering").unwrap();
        for key in [
            "ignore",
            "select",
            "extend-select",
            "error",
            "warn",
            "exclude",
            "baseline",
            "min-confidence",
        ] {
            assert!(
                filtering.path(key).is_dir(),
                "missing filtering fixture: {key}"
            );
            assert!(filtering.path(key).join("lsp-settings.json").is_file());
        }

        let package = FixtureProject::from_fixture("complete-package").unwrap();
        for path in [
            "DESCRIPTION",
            "NAMESPACE",
            "R/imports.R",
            "R/native.R",
            "typesheds/fixturedep.json",
            "data/ordinary.rda",
            "data/oversized.rda",
        ] {
            assert!(
                package.path(path).is_file(),
                "missing package fixture: {path}"
            );
        }
        assert!(
            fs::metadata(package.path("data/ordinary.rda"))
                .unwrap()
                .len()
                <= 128
        );
        assert!(
            fs::metadata(package.path("data/oversized.rda"))
                .unwrap()
                .len()
                > 128
        );

        let excluded = FixtureProject::from_fixture("excluded-influence").unwrap();
        assert!(excluded.path("excluded.R").is_file());
        assert!(excluded.path("kept.R").is_file());

        let unicode = FixtureProject::from_fixture("unicode").unwrap();
        let source = fs::read_to_string(unicode.path("R/non_ascii.R")).unwrap();
        assert!(source.contains('é') && source.contains('😀') && source.contains("e\u{301}"));

        let roots = FixtureProject::from_fixture("multi-root").unwrap();
        for root in ["root-a", "root-b"] {
            assert!(roots.path(root).join("ry.toml").is_file());
            assert!(roots.path(root).join("stubs/local.json").is_file());
        }
        assert_ne!(
            fs::read(roots.path("root-a/ry.toml")).unwrap(),
            fs::read(roots.path("root-b/ry.toml")).unwrap()
        );
    }

    #[test]
    fn builder_rejects_paths_outside_the_fixture() {
        let fixture = FixtureProject::empty().unwrap();
        assert!(fixture.write_file("../outside", "no").is_err());
    }
}
