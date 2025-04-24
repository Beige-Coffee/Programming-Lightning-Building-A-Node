use std::fs;
use lightning::io::{self, Error, ErrorKind};
use std::path::{Path, PathBuf};
use lightning::util::persist::KVStore;
use std::io::Read;
use std::io::Write;


/// A [`KVStore`] implementation that writes to and reads from the file system.
pub struct FileStore {
  data_dir: PathBuf,
}

impl FileStore {
  /// Create a new FileStore with the given base directory
  pub fn new(data_dir: PathBuf) -> io::Result<Self> {
      fs::create_dir_all(&data_dir)?;
      Ok(Self { data_dir })
  }

  /// Helper function to build the full path for a key
  fn get_path(&self, primary: &str, secondary: &str, key: &str) -> PathBuf {
      let mut path = self.data_dir.clone();
      path.push(primary);
      if !secondary.is_empty() {
          path.push(secondary);
      }
      path.push(key);
      path
  }
}


impl KVStore for FileStore {
  fn read(&self, primary_namespace: &str, secondary_namespace: &str, key: &str) -> Result<Vec<u8>, io::Error> {
      let path = self.get_path(primary_namespace, secondary_namespace, key);

      // If the file doesn't exist, return NotFound error
      if !path.exists() {
          return Err(io::Error::new(io::ErrorKind::NotFound, "Key not found"));
      }

      let mut buffer = Vec::new();
      let mut file = fs::File::open(path)?;
      file.read_to_end(&mut buffer)?;
      Ok(buffer)
  }

  fn write(&self, primary_namespace: &str, secondary_namespace: &str, key: &str, buf: &[u8]) -> Result<(), io::Error> {
      let path = self.get_path(primary_namespace, secondary_namespace, key);

      if let Some(parent) = path.parent() {
          fs::create_dir_all(parent)?;
      }

      let mut file = fs::File::create(path)?;
      file.write_all(buf)?;
      file.sync_all()?;
      Ok(())
  }

  fn remove(&self, primary_namespace: &str, secondary_namespace: &str, key: &str, lazy: bool) -> Result<(), io::Error> {
      let path = self.get_path(primary_namespace, secondary_namespace, key);

      // For this simple implementation, we ignore the lazy flag
      // In a production implementation, you might want to handle lazy deletion differently
      if path.exists() {
          fs::remove_file(path)?;
      }
      Ok(())
  }

  fn list(&self, primary_namespace: &str, secondary_namespace: &str) -> Result<Vec<String>, io::Error> {
      let mut dir_path = self.data_dir.clone();
      dir_path.push(primary_namespace);
      if !secondary_namespace.is_empty() {
          dir_path.push(secondary_namespace);
      }

      if !dir_path.exists() {
          return Ok(Vec::new());
      }

      let mut keys = Vec::new();
      for entry in fs::read_dir(dir_path)? {
          let entry = entry?;
          if entry.file_type()?.is_file() {
              if let Some(name) = entry.file_name().to_str() {
                  keys.push(name.to_string());
              }
          }
      }
      Ok(keys)
  }
}
