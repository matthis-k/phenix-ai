//! SQLite-backed Basic Storage. Session and journal semantics stay in their
//! own plugins. SQLite serializes CAS transactions across provider processes.
//!
//! PHENIX_BASIC_SQLITE_PATH is set by the product launcher. No default database
//! is created in the current directory, and a missing setting fails closed.
use phenix_basic_contracts::{
    error, BasicError, ErrorCode, RecordKey, StorageEdit, StorageTransaction, StoredRecord,
};
use phenix_sdk::Blob;
use std::{
    collections::BTreeSet,
    env,
    ffi::{c_char, c_int, c_void, CString},
    marker::PhantomData,
    ptr::{self, NonNull},
};

#[allow(non_camel_case_types)]
mod ffi {
    use super::{c_char, c_int, c_void};
    #[repr(C)]
    pub struct sqlite3 { _private: [u8; 0] }
    #[repr(C)]
    pub struct sqlite3_stmt { _private: [u8; 0] }
    #[link(name = "sqlite3")]
    unsafe extern "C" {
        pub fn sqlite3_open_v2(path: *const c_char, db: *mut *mut sqlite3, flags: c_int, vfs: *const c_char) -> c_int;
        pub fn sqlite3_close(db: *mut sqlite3) -> c_int;
        pub fn sqlite3_busy_timeout(db: *mut sqlite3, millis: c_int) -> c_int;
        pub fn sqlite3_exec(db: *mut sqlite3, sql: *const c_char, callback: *mut c_void, arg: *mut c_void, err: *mut *mut c_char) -> c_int;
        pub fn sqlite3_free(ptr: *mut c_void);
        pub fn sqlite3_prepare_v2(db: *mut sqlite3, sql: *const c_char, n: c_int, stmt: *mut *mut sqlite3_stmt, tail: *mut *const c_char) -> c_int;
        pub fn sqlite3_finalize(stmt: *mut sqlite3_stmt) -> c_int;
        pub fn sqlite3_bind_text(stmt: *mut sqlite3_stmt, idx: c_int, data: *const c_char, size: c_int, destructor: Option<unsafe extern "C" fn(*mut c_void)>) -> c_int;
        pub fn sqlite3_bind_blob(stmt: *mut sqlite3_stmt, idx: c_int, data: *const c_void, size: c_int, destructor: Option<unsafe extern "C" fn(*mut c_void)>) -> c_int;
        pub fn sqlite3_bind_int64(stmt: *mut sqlite3_stmt, idx: c_int, value: i64) -> c_int;
        pub fn sqlite3_step(stmt: *mut sqlite3_stmt) -> c_int;
        pub fn sqlite3_column_int64(stmt: *mut sqlite3_stmt, idx: c_int) -> i64;
        pub fn sqlite3_column_blob(stmt: *mut sqlite3_stmt, idx: c_int) -> *const c_void;
        pub fn sqlite3_column_bytes(stmt: *mut sqlite3_stmt, idx: c_int) -> c_int;
    }
}
const OK: c_int = 0;
const ROW: c_int = 100;
const DONE: c_int = 101;
const OPEN_READWRITE_CREATE_FULLMUTEX: c_int = 0x2 | 0x4 | 0x10000;
const CREATE: &str = "CREATE TABLE IF NOT EXISTS basic_records (session TEXT NOT NULL, name TEXT NOT NULL, revision INTEGER NOT NULL CHECK (revision > 0), data BLOB NOT NULL, PRIMARY KEY (session, name)) WITHOUT ROWID";
const SELECT: &str = "SELECT revision, data FROM basic_records WHERE session = ?1 AND name = ?2";
const UPSERT: &str = "INSERT INTO basic_records (session, name, revision, data) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(session, name) DO UPDATE SET revision = excluded.revision, data = excluded.data";

fn failed() -> BasicError {
    error(ErrorCode::StorageFailed, "SQLite storage operation failed")
}
fn bad_key() -> BasicError {
    error(ErrorCode::InvalidInput, "invalid storage transaction key")
}
fn checked_i64(value: u64) -> Result<i64, BasicError> {
    i64::try_from(value).map_err(|_| error(ErrorCode::Conflict, "storage revision overflow"))
}
fn cstr(input: &str) -> Result<CString, BasicError> {
    CString::new(input).map_err(|_| failed())
}

struct Db(NonNull<ffi::sqlite3>);
impl Db {
    fn open() -> Result<Self, BasicError> {
        let name = env::var("PHENIX_BASIC_SQLITE_PATH").map_err(|_| failed())?;
        if name.is_empty() || name == ":memory:" || name.starts_with("file:") {
            return Err(failed());
        }
        let path = cstr(&name)?;
        let mut raw = ptr::null_mut();
        // SQLite allocates a handle even when opening fails.
        let status = unsafe {
            ffi::sqlite3_open_v2(path.as_ptr(), &mut raw, OPEN_READWRITE_CREATE_FULLMUTEX, ptr::null())
        };
        if status != OK {
            if !raw.is_null() {
                unsafe { ffi::sqlite3_close(raw) };
            }
            return Err(failed());
        }
        let db = Self(NonNull::new(raw).ok_or_else(failed)?);
        if unsafe { ffi::sqlite3_busy_timeout(db.0.as_ptr(), 5000) } != OK {
            return Err(failed());
        }
        db.exec("PRAGMA synchronous = FULL")?;
        db.exec(CREATE)?;
        Ok(db)
    }

    fn exec(&self, sql: &str) -> Result<(), BasicError> {
        let sql = cstr(sql)?;
        let mut err = ptr::null_mut();
        let status = unsafe {
            ffi::sqlite3_exec(self.0.as_ptr(), sql.as_ptr(), ptr::null_mut(), ptr::null_mut(), &mut err)
        };
        if !err.is_null() {
            unsafe { ffi::sqlite3_free(err.cast()) };
        }
        if status == OK { Ok(()) } else { Err(failed()) }
    }

    fn stmt(&self, sql: &str) -> Result<Stmt<'_>, BasicError> {
        let sql = cstr(sql)?;
        let mut raw = ptr::null_mut();
        let status = unsafe {
            ffi::sqlite3_prepare_v2(self.0.as_ptr(), sql.as_ptr(), -1, &mut raw, ptr::null_mut())
        };
        if status != OK {
            return Err(failed());
        }
        Ok(Stmt {
            ptr: NonNull::new(raw).ok_or_else(failed)?,
            _db: PhantomData,
        })
    }

    fn get(&self, key: &RecordKey) -> Result<Option<StoredRecord>, BasicError> {
        let stmt = self.stmt(SELECT)?;
        stmt.text(1, &key.session)?;
        stmt.text(2, &key.key)?;
        match stmt.step()? {
            ROW => {
                let revision = unsafe { ffi::sqlite3_column_int64(stmt.ptr.as_ptr(), 0) };
                let revision = u64::try_from(revision).map_err(|_| failed())?;
                if revision == 0 {
                    return Err(failed());
                }
                let len = unsafe { ffi::sqlite3_column_bytes(stmt.ptr.as_ptr(), 1) };
                if len < 0 {
                    return Err(failed());
                }
                let value = unsafe { ffi::sqlite3_column_blob(stmt.ptr.as_ptr(), 1) };
                if len > 0 && value.is_null() {
                    return Err(failed());
                }
                let bytes = if len == 0 {
                    Vec::new()
                } else {
                    unsafe { std::slice::from_raw_parts(value.cast::<u8>(), len as usize).to_vec() }
                };
                Ok(Some(StoredRecord { revision, data: Blob(bytes) }))
            }
            DONE => Ok(None),
            _ => Err(failed()),
        }
    }

    fn put(&self, key: &RecordKey, record: &StoredRecord) -> Result<(), BasicError> {
        let stmt = self.stmt(UPSERT)?;
        stmt.text(1, &key.session)?;
        stmt.text(2, &key.key)?;
        stmt.number(3, checked_i64(record.revision)?)?;
        stmt.blob(4, &record.data.0)?;
        if stmt.step()? == DONE { Ok(()) } else { Err(failed()) }
    }
}
impl Drop for Db {
    fn drop(&mut self) {
        unsafe { ffi::sqlite3_close(self.0.as_ptr()) };
    }
}

struct Stmt<'a> {
    ptr: NonNull<ffi::sqlite3_stmt>,
    _db: PhantomData<&'a Db>,
}
impl Stmt<'_> {
    fn text(&self, index: c_int, value: &str) -> Result<(), BasicError> {
        let len = c_int::try_from(value.len()).map_err(|_| bad_key())?;
        // SQLITE_STATIC: request strings remain alive until step and finalize.
        let status = unsafe {
            ffi::sqlite3_bind_text(self.ptr.as_ptr(), index, value.as_ptr().cast(), len, None)
        };
        if status == OK { Ok(()) } else { Err(failed()) }
    }
    fn blob(&self, index: c_int, value: &[u8]) -> Result<(), BasicError> {
        static EMPTY: u8 = 0;
        let len = c_int::try_from(value.len()).map_err(|_| bad_key())?;
        let raw = if value.is_empty() { &EMPTY as *const u8 } else { value.as_ptr() };
        let status = unsafe {
            ffi::sqlite3_bind_blob(self.ptr.as_ptr(), index, raw.cast(), len, None)
        };
        if status == OK { Ok(()) } else { Err(failed()) }
    }
    fn number(&self, index: c_int, value: i64) -> Result<(), BasicError> {
        if unsafe { ffi::sqlite3_bind_int64(self.ptr.as_ptr(), index, value) } == OK {
            Ok(())
        } else {
            Err(failed())
        }
    }
    fn step(&self) -> Result<c_int, BasicError> {
        match unsafe { ffi::sqlite3_step(self.ptr.as_ptr()) } {
            ROW => Ok(ROW),
            DONE => Ok(DONE),
            _ => Err(failed()),
        }
    }
}
impl Drop for Stmt<'_> {
    fn drop(&mut self) {
        unsafe { ffi::sqlite3_finalize(self.ptr.as_ptr()) };
    }
}

/// Check the whole edit set before writing. BEGIN IMMEDIATE provides an
/// inter-process CAS boundary; failures roll back every edit in the batch.
fn transact(request: StorageTransaction) -> Result<Vec<StoredRecord>, BasicError> {
    let mut keys = BTreeSet::new();
    for edit in &request.edits {
        if edit.key.is_empty() || !keys.insert(edit.key.as_str()) {
            return Err(bad_key());
        }
        checked_i64(edit.expected.checked_add(1).ok_or_else(|| {
            error(ErrorCode::Conflict, "storage revision overflow")
        })?)?;
    }
    if request.session.is_empty() {
        return Err(bad_key());
    }
    let db = Db::open()?;
    db.exec("BEGIN IMMEDIATE")?;
    let result = (|| {
        let mut records = Vec::with_capacity(request.edits.len());
        for StorageEdit { key, expected, data } in request.edits {
            let record_key = RecordKey { session: request.session.clone(), key };
            let current = db.get(&record_key)?;
            let revision = current.as_ref().map_or(0, |record| record.revision);
            if revision != expected {
                return Err(error(ErrorCode::Conflict, "storage revision conflict"));
            }
            let record = StoredRecord { revision: revision + 1, data };
            db.put(&record_key, &record)?;
            records.push(record);
        }
        Ok(records)
    })();
    match result {
        Ok(records) => {
            if let Err(err) = db.exec("COMMIT") {
                let _ = db.exec("ROLLBACK");
                Err(err)
            } else {
                Ok(records)
            }
        }
        Err(err) => {
            let _ = db.exec("ROLLBACK");
            Err(err)
        }
    }
}

#[phenix_sdk::plugin(
    id = "phenix.basic.storage-sqlite",
    version = "1.0.0",
    provide(phenix_basic_contracts::Storage, slot = "Storage")
)]
mod plugin {
    use super::{transact, Db};
    use phenix_basic_contracts::{
        error, BasicError, ErrorCode, RecordKey, Storage, StorageTransaction, StoredRecord,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State;

    #[provide(Storage::read)]
    fn read(
        _: &mut State,
        host: &Host<'_>,
        request: RecordKey,
    ) -> CallResult<StoredRecord, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        if request.key.is_empty() {
            return Err(CallError::Domain(error(ErrorCode::InvalidInput, "empty storage key")));
        }
        Db::open()
            .and_then(|db| db.get(&request))
            .and_then(|row| row.ok_or_else(|| error(ErrorCode::NotFound, "missing record")))
            .map_err(CallError::Domain)
    }

    #[provide(Storage::transact)]
    fn write(
        _: &mut State,
        host: &Host<'_>,
        request: StorageTransaction,
    ) -> CallResult<Vec<StoredRecord>, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        transact(request).map_err(CallError::Domain)
    }
}
