use lightning::util::persist::KVStore;
use lightning::util::persist::{KVSTORE_NAMESPACE_KEY_ALPHABET, KVSTORE_NAMESPACE_KEY_MAX_LEN};
use lightning::util::string::PrintableString;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

// The number of read/write/remove/list operations after which we clean up our `locks` HashMap.
const GC_LOCK_INTERVAL: usize = 25;

/// A [`KVStore`] implementation that writes to and reads from the file system.
pub struct FilesystemStore {
	data_dir: PathBuf,
	tmp_file_counter: AtomicUsize,
	gc_counter: AtomicUsize,
	locks: Mutex<HashMap<PathBuf, Arc<RwLock<()>>>>,
}

impl FilesystemStore {
	/// Constructs a new [`FilesystemStore`].
	pub fn new(data_dir: PathBuf) -> Self {
		let locks = Mutex::new(HashMap::new());
		let tmp_file_counter = AtomicUsize::new(0);
		let gc_counter = AtomicUsize::new(1);
		Self { data_dir, tmp_file_counter, gc_counter, locks }
	}

	/// Returns the data directory.
	pub fn get_data_dir(&self) -> PathBuf {
		self.data_dir.clone()
	}

	fn garbage_collect_locks(&self) {
		let gc_counter = self.gc_counter.fetch_add(1, Ordering::AcqRel);

		if gc_counter % GC_LOCK_INTERVAL == 0 {
			// Take outer lock for the cleanup.
			let mut outer_lock = self.locks.lock().unwrap();

			// Garbage collect all lock entries that are not referenced anymore.
			outer_lock.retain(|_, v| Arc::strong_count(&v) > 1);
		}
	}

	fn get_dest_dir_path(
		&self, primary_namespace: &str, secondary_namespace: &str,
	) -> std::io::Result<PathBuf> {
		let mut dest_dir_path = self.data_dir.clone();

		dest_dir_path.push(primary_namespace);
		if !secondary_namespace.is_empty() {
			dest_dir_path.push(secondary_namespace);
		}

		Ok(dest_dir_path)
	}
}

impl KVStore for FilesystemStore {
	fn read(
		&self, primary_namespace: &str, secondary_namespace: &str, key: &str,
	) -> lightning::io::Result<Vec<u8>> {
		check_namespace_key_validity(primary_namespace, secondary_namespace, Some(key), "read")?;

		let mut dest_file_path = self.get_dest_dir_path(primary_namespace, secondary_namespace)?;
		dest_file_path.push(key);

		let mut buf = Vec::new();
		{
			let inner_lock_ref = {
				let mut outer_lock = self.locks.lock().unwrap();
				Arc::clone(&outer_lock.entry(dest_file_path.clone()).or_default())
			};
			let _guard = inner_lock_ref.read().unwrap();

			let mut f = fs::File::open(dest_file_path)?;
			f.read_to_end(&mut buf)?;
		}

		self.garbage_collect_locks();

		Ok(buf)
	}


	////////////////////////////
	// START Exercise 8 //
	// Implement write for our FilesystemStore
	////////////////////////////

	fn write(
		&self, primary_namespace: &str, secondary_namespace: &str, key: &str, buf: &[u8],
	) -> lightning::io::Result<()> {
		check_namespace_key_validity(primary_namespace, secondary_namespace, Some(key), "write")?;

		let mut dest_file_path = self.get_dest_dir_path(primary_namespace, secondary_namespace)?;
		dest_file_path.push(key);

		let parent_directory = dest_file_path.parent().ok_or_else(|| {
			let msg =
				format!("Could not retrieve parent directory of {}.", dest_file_path.display());
			std::io::Error::new(std::io::ErrorKind::InvalidInput, msg)
		})?;
		fs::create_dir_all(&parent_directory)?;

		// Do a crazy dance with lots of fsync()s to be overly cautious here...
		// We never want to end up in a state where we've lost the old data, or end up using the
		// old data on power loss after we've returned.
		// The way to atomically write a file on Unix platforms is:
		// open(tmpname), write(tmpfile), fsync(tmpfile), close(tmpfile), rename(), fsync(dir)
		let mut tmp_file_path = dest_file_path.clone();
		let tmp_file_ext = format!("{}.tmp", self.tmp_file_counter.fetch_add(1, Ordering::AcqRel));
		tmp_file_path.set_extension(tmp_file_ext);

		{
			let mut tmp_file = fs::File::create(&tmp_file_path)?;
			tmp_file.write_all(&buf)?;
			tmp_file.sync_all()?;
		}

		let res = {
			let inner_lock_ref = {
				let mut outer_lock = self.locks.lock().unwrap();
				Arc::clone(&outer_lock.entry(dest_file_path.clone()).or_default())
			};
			let _guard = inner_lock_ref.write().unwrap();

			fs::rename(&tmp_file_path, &dest_file_path)?;
			let dir_file = fs::OpenOptions::new().read(true).open(&parent_directory)?;
			dir_file.sync_all()?;
			Ok(())
		};

		self.garbage_collect_locks();

		res
	}

	fn remove(
		&self, primary_namespace: &str, secondary_namespace: &str, key: &str, lazy: bool,
	) -> lightning::io::Result<()> {
		check_namespace_key_validity(primary_namespace, secondary_namespace, Some(key), "remove")?;

		let mut dest_file_path = self.get_dest_dir_path(primary_namespace, secondary_namespace)?;
		dest_file_path.push(key);

		if !dest_file_path.is_file() {
			return Ok(());
		}

		{
			let inner_lock_ref = {
				let mut outer_lock = self.locks.lock().unwrap();
				Arc::clone(&outer_lock.entry(dest_file_path.clone()).or_default())
			};
			let _guard = inner_lock_ref.write().unwrap();

			if lazy {
				// If we're lazy we just call remove and be done with it.
				fs::remove_file(&dest_file_path)?;
			} else {
				// If we're not lazy we try our best to persist the updated metadata to ensure
				// atomicity of this call.

				fs::remove_file(&dest_file_path)?;

				let parent_directory = dest_file_path.parent().ok_or_else(|| {
					let msg = format!(
						"Could not retrieve parent directory of {}.",
						dest_file_path.display()
					);
					std::io::Error::new(std::io::ErrorKind::InvalidInput, msg)
				})?;
				let dir_file = fs::OpenOptions::new().read(true).open(parent_directory)?;
				// The above call to `fs::remove_file` corresponds to POSIX `unlink`, whose changes
				// to the inode might get cached (and hence possibly lost on crash), depending on
				// the target platform and file system.
				//
				// In order to assert we permanently removed the file in question we therefore
				// call `fsync` on the parent directory on platforms that support it.
				dir_file.sync_all()?;
			}
		}

		self.garbage_collect_locks();

		Ok(())
	}

	fn list(
		&self, primary_namespace: &str, secondary_namespace: &str,
	) -> lightning::io::Result<Vec<String>> {
		check_namespace_key_validity(primary_namespace, secondary_namespace, None, "list")?;

		let prefixed_dest = self.get_dest_dir_path(primary_namespace, secondary_namespace)?;
		let mut keys = Vec::new();

		if !Path::new(&prefixed_dest).exists() {
			return Ok(Vec::new());
		}

		for entry in fs::read_dir(&prefixed_dest)? {
			let entry = entry?;
			let p = entry.path();

			if !dir_entry_is_key(&p)? {
				continue;
			}

			let key = get_key_from_dir_entry(&p, &prefixed_dest)?;

			keys.push(key);
		}

		self.garbage_collect_locks();

		Ok(keys)
	}
}

fn dir_entry_is_key(p: &Path) -> Result<bool, lightning::io::Error> {
	if let Some(ext) = p.extension() {
		if ext == "tmp" {
			return Ok(false);
		}
	}

	let metadata = p.metadata().map_err(|e| {
		let msg = format!(
			"Failed to list keys at path {}: {}",
			PrintableString(p.to_str().unwrap_or_default()),
			e
		);
		lightning::io::Error::new(lightning::io::ErrorKind::Other, msg)
	})?;

	// We allow the presence of directories in the empty primary namespace and just skip them.
	if metadata.is_dir() {
		return Ok(false);
	}

	// If we otherwise don't find a file at the given path something went wrong.
	if !metadata.is_file() {
		debug_assert!(
			false,
			"Failed to list keys at path {}: file couldn't be accessed.",
			PrintableString(p.to_str().unwrap_or_default())
		);
		let msg = format!(
			"Failed to list keys at path {}: file couldn't be accessed.",
			PrintableString(p.to_str().unwrap_or_default())
		);
		return Err(lightning::io::Error::new(lightning::io::ErrorKind::Other, msg));
	}

	Ok(true)
}

fn get_key_from_dir_entry(p: &Path, base_path: &Path) -> Result<String, lightning::io::Error> {
	match p.strip_prefix(&base_path) {
		Ok(stripped_path) => {
			if let Some(relative_path) = stripped_path.to_str() {
				if is_valid_kvstore_str(relative_path) {
					return Ok(relative_path.to_string());
				} else {
					debug_assert!(
						false,
						"Failed to list keys of path {}: file path is not valid key",
						PrintableString(p.to_str().unwrap_or_default())
					);
					let msg = format!(
						"Failed to list keys of path {}: file path is not valid key",
						PrintableString(p.to_str().unwrap_or_default())
					);
					return Err(lightning::io::Error::new(lightning::io::ErrorKind::Other, msg));
				}
			} else {
				debug_assert!(
					false,
					"Failed to list keys of path {}: file path is not valid UTF-8",
					PrintableString(p.to_str().unwrap_or_default())
				);
				let msg = format!(
					"Failed to list keys of path {}: file path is not valid UTF-8",
					PrintableString(p.to_str().unwrap_or_default())
				);
				return Err(lightning::io::Error::new(lightning::io::ErrorKind::Other, msg));
			}
		},
		Err(e) => {
			debug_assert!(
				false,
				"Failed to list keys of path {}: {}",
				PrintableString(p.to_str().unwrap_or_default()),
				e
			);
			let msg = format!(
				"Failed to list keys of path {}: {}",
				PrintableString(p.to_str().unwrap_or_default()),
				e
			);
			return Err(lightning::io::Error::new(lightning::io::ErrorKind::Other, msg));
		},
	}
}

fn is_valid_kvstore_str(key: &str) -> bool {
	key.len() <= KVSTORE_NAMESPACE_KEY_MAX_LEN
		&& key.chars().all(|c| KVSTORE_NAMESPACE_KEY_ALPHABET.contains(c))
}

fn check_namespace_key_validity(
	primary_namespace: &str, secondary_namespace: &str, key: Option<&str>, operation: &str,
) -> Result<(), std::io::Error> {
	if let Some(key) = key {
		if key.is_empty() {
			debug_assert!(
				false,
				"Failed to {} {}/{}/{}: key may not be empty.",
				operation,
				PrintableString(primary_namespace),
				PrintableString(secondary_namespace),
				PrintableString(key)
			);
			let msg = format!(
				"Failed to {} {}/{}/{}: key may not be empty.",
				operation,
				PrintableString(primary_namespace),
				PrintableString(secondary_namespace),
				PrintableString(key)
			);
			return Err(std::io::Error::new(std::io::ErrorKind::Other, msg));
		}

		if primary_namespace.is_empty() && !secondary_namespace.is_empty() {
			debug_assert!(false,
                "Failed to {} {}/{}/{}: primary namespace may not be empty if a non-empty secondary namespace is given.",
                operation,
                PrintableString(primary_namespace), PrintableString(secondary_namespace), PrintableString(key));
			let msg = format!(
                "Failed to {} {}/{}/{}: primary namespace may not be empty if a non-empty secondary namespace is given.", operation,
                PrintableString(primary_namespace), PrintableString(secondary_namespace), PrintableString(key));
			return Err(std::io::Error::new(std::io::ErrorKind::Other, msg));
		}

		if !is_valid_kvstore_str(primary_namespace)
			|| !is_valid_kvstore_str(secondary_namespace)
			|| !is_valid_kvstore_str(key)
		{
			debug_assert!(false, "Failed to {} {}/{}/{}: primary namespace, secondary namespace, and key must be valid.",
                operation,
                PrintableString(primary_namespace), PrintableString(secondary_namespace), PrintableString(key));
			let msg = format!("Failed to {} {}/{}/{}: primary namespace, secondary namespace, and key must be valid.",
                operation,
                PrintableString(primary_namespace), PrintableString(secondary_namespace), PrintableString(key));
			return Err(std::io::Error::new(std::io::ErrorKind::Other, msg));
		}
	} else {
		if primary_namespace.is_empty() && !secondary_namespace.is_empty() {
			debug_assert!(false,
                "Failed to {} {}/{}: primary namespace may not be empty if a non-empty secondary namespace is given.",
                operation, PrintableString(primary_namespace), PrintableString(secondary_namespace));
			let msg = format!(
                "Failed to {} {}/{}: primary namespace may not be empty if a non-empty secondary namespace is given.",
                operation, PrintableString(primary_namespace), PrintableString(secondary_namespace));
			return Err(std::io::Error::new(std::io::ErrorKind::Other, msg));
		}
		if !is_valid_kvstore_str(primary_namespace) || !is_valid_kvstore_str(secondary_namespace) {
			debug_assert!(
				false,
				"Failed to {} {}/{}: primary namespace and secondary namespace must be valid.",
				operation,
				PrintableString(primary_namespace),
				PrintableString(secondary_namespace)
			);
			let msg = format!(
				"Failed to {} {}/{}: primary namespace and secondary namespace must be valid.",
				operation,
				PrintableString(primary_namespace),
				PrintableString(secondary_namespace)
			);
			return Err(std::io::Error::new(std::io::ErrorKind::Other, msg));
		}
	}

	Ok(())
}
