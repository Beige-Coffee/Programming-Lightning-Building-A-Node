pub struct KeysManager(String);

impl KeysManager {
	pub fn new() -> Self {
		KeysManager(String::new())
	}
}

pub struct PeerManager(String);

impl PeerManager {
	pub fn new() -> Self {
		PeerManager(String::new())
	}
}

pub struct FileStore(String);

impl FileStore {
	pub fn new() -> Self {
		FileStore(String::new())
	}
}
