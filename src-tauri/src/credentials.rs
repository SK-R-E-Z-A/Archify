//! Secure storage for provider-profile credentials.

#[cfg(test)]
use std::collections::HashMap;
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

const SERVICE_NAME: &str = "com.archify.provider-profile";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialStoreError {
    Unavailable,
    OperationFailed,
}

impl CredentialStoreError {
    pub fn safe_message(self) -> &'static str {
        match self {
            Self::Unavailable => "Secure credential storage is unavailable on this platform.",
            Self::OperationFailed => "Secure credential storage could not complete the operation.",
        }
    }
}

pub trait CredentialStore: Send + Sync {
    fn store(&self, profile_id: &str, secret: &str) -> Result<(), CredentialStoreError>;
    fn retrieve(&self, profile_id: &str) -> Result<Option<String>, CredentialStoreError>;
    fn delete(&self, profile_id: &str) -> Result<(), CredentialStoreError>;

    fn contains(&self, profile_id: &str) -> Result<bool, CredentialStoreError> {
        self.retrieve(profile_id)
            .map(|credential| credential.is_some())
    }
}

pub struct UnavailableCredentialStore;

impl CredentialStore for UnavailableCredentialStore {
    fn store(&self, _profile_id: &str, _secret: &str) -> Result<(), CredentialStoreError> {
        Err(CredentialStoreError::Unavailable)
    }

    fn retrieve(&self, _profile_id: &str) -> Result<Option<String>, CredentialStoreError> {
        Err(CredentialStoreError::Unavailable)
    }

    fn delete(&self, _profile_id: &str) -> Result<(), CredentialStoreError> {
        Err(CredentialStoreError::Unavailable)
    }
}

#[cfg(windows)]
pub struct WindowsCredentialStore;

#[cfg(windows)]
impl WindowsCredentialStore {
    fn entry(profile_id: &str) -> Result<keyring::Entry, CredentialStoreError> {
        keyring::Entry::new(SERVICE_NAME, profile_id)
            .map_err(|_| CredentialStoreError::OperationFailed)
    }
}

#[cfg(windows)]
impl CredentialStore for WindowsCredentialStore {
    fn store(&self, profile_id: &str, secret: &str) -> Result<(), CredentialStoreError> {
        Self::entry(profile_id)?
            .set_password(secret)
            .map_err(|_| CredentialStoreError::OperationFailed)
    }

    fn retrieve(&self, profile_id: &str) -> Result<Option<String>, CredentialStoreError> {
        match Self::entry(profile_id)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(CredentialStoreError::OperationFailed),
        }
    }

    fn delete(&self, profile_id: &str) -> Result<(), CredentialStoreError> {
        match Self::entry(profile_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(CredentialStoreError::OperationFailed),
        }
    }
}

#[cfg(windows)]
pub fn system_credential_store() -> Arc<dyn CredentialStore> {
    Arc::new(WindowsCredentialStore)
}

#[cfg(not(windows))]
pub fn system_credential_store() -> Arc<dyn CredentialStore> {
    Arc::new(UnavailableCredentialStore)
}

#[cfg(test)]
pub fn default_credential_store() -> Arc<dyn CredentialStore> {
    Arc::new(TestCredentialStore)
}

#[cfg(not(test))]
pub fn default_credential_store() -> Arc<dyn CredentialStore> {
    system_credential_store()
}

#[cfg(test)]
struct TestCredentialStore;

#[cfg(test)]
impl CredentialStore for TestCredentialStore {
    fn store(&self, _profile_id: &str, _secret: &str) -> Result<(), CredentialStoreError> {
        Err(CredentialStoreError::Unavailable)
    }

    fn retrieve(&self, _profile_id: &str) -> Result<Option<String>, CredentialStoreError> {
        Ok(None)
    }

    fn delete(&self, _profile_id: &str) -> Result<(), CredentialStoreError> {
        Ok(())
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct MemoryCredentialStore {
    values: Mutex<HashMap<String, String>>,
}

#[cfg(test)]
impl CredentialStore for MemoryCredentialStore {
    fn store(&self, profile_id: &str, secret: &str) -> Result<(), CredentialStoreError> {
        self.values
            .lock()
            .unwrap()
            .insert(profile_id.to_string(), secret.to_string());
        Ok(())
    }

    fn retrieve(&self, profile_id: &str) -> Result<Option<String>, CredentialStoreError> {
        Ok(self.values.lock().unwrap().get(profile_id).cloned())
    }

    fn delete(&self, profile_id: &str) -> Result<(), CredentialStoreError> {
        self.values.lock().unwrap().remove(profile_id);
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod windows_credential_store_tests {
    use super::{CredentialStore, WindowsCredentialStore};
    use uuid::Uuid;

    struct CredentialCleanup {
        store: WindowsCredentialStore,
        profile_ids: Vec<String>,
    }

    impl CredentialCleanup {
        fn new() -> Self {
            Self {
                store: WindowsCredentialStore,
                profile_ids: Vec::new(),
            }
        }

        fn unique_profile_id(&mut self) -> String {
            let profile_id = format!("native-test-{}", Uuid::new_v4());
            self.profile_ids.push(profile_id.clone());
            profile_id
        }
    }

    impl Drop for CredentialCleanup {
        fn drop(&mut self) {
            for profile_id in &self.profile_ids {
                let _ = self.store.delete(profile_id);
            }
        }
    }

    #[test]
    fn windows_credential_store_scopes_replaces_and_deletes_profile_credentials() {
        let mut cleanup = CredentialCleanup::new();
        let first_profile = cleanup.unique_profile_id();
        let second_profile = cleanup.unique_profile_id();
        let store = &cleanup.store;

        let first_secret = "native-test-secret-first";
        let replacement_secret = "native-test-secret-replacement";
        let second_secret = "native-test-secret-second";

        assert!(
            store
                .retrieve(&first_profile)
                .expect("retrieve first profile")
                .is_none()
        );
        assert!(
            store
                .retrieve(&second_profile)
                .expect("retrieve second profile")
                .is_none()
        );

        store
            .store(&first_profile, first_secret)
            .expect("store first profile credential");
        store
            .store(&second_profile, second_secret)
            .expect("store second profile credential");
        assert!(
            store
                .retrieve(&first_profile)
                .expect("retrieve first profile")
                .as_deref()
                == Some(first_secret),
            "first profile credential did not round-trip"
        );
        assert!(
            store
                .retrieve(&second_profile)
                .expect("retrieve second profile")
                .as_deref()
                == Some(second_secret),
            "second profile credential did not round-trip"
        );

        store
            .store(&first_profile, replacement_secret)
            .expect("replace first profile credential");
        assert!(
            store
                .retrieve(&first_profile)
                .expect("retrieve replaced credential")
                .as_deref()
                == Some(replacement_secret),
            "replacement credential did not round-trip"
        );
        assert!(
            store
                .retrieve(&second_profile)
                .expect("verify second profile isolation")
                .as_deref()
                == Some(second_secret),
            "replacing one profile credential affected another profile"
        );

        store
            .delete(&first_profile)
            .expect("delete first profile credential");
        assert!(
            store
                .retrieve(&first_profile)
                .expect("verify first profile deletion")
                .is_none(),
            "deleted credential is still present"
        );
        assert!(
            store
                .retrieve(&second_profile)
                .expect("verify second profile remains")
                .as_deref()
                == Some(second_secret),
            "deleting one profile credential affected another profile"
        );
        store
            .delete(&second_profile)
            .expect("delete second profile credential");
    }
}

#[cfg(all(test, not(windows)))]
mod non_windows_credential_store_tests {
    use super::{CredentialStore, CredentialStoreError, UnavailableCredentialStore};

    #[test]
    fn system_credential_store_is_unavailable_without_plaintext_fallback() {
        let store = UnavailableCredentialStore;
        assert_eq!(
            store.store("test-profile", "test-secret"),
            Err(CredentialStoreError::Unavailable)
        );
        assert_eq!(
            store.retrieve("test-profile"),
            Err(CredentialStoreError::Unavailable)
        );
        assert_eq!(
            store.delete("test-profile"),
            Err(CredentialStoreError::Unavailable)
        );
    }
}
