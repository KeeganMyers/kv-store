use chrono::{DateTime, Duration, Utc};
use evmap::{ReadHandle, ReadHandleFactory, WriteHandle};
use parking_lot::Mutex;
use priority_queue::double_priority_queue::DoublePriorityQueue;
use serde_json::{json, value::Value};
use std::{error::Error, fmt, sync::Arc};
use tokio::{task, task::JoinHandle};

#[derive(Debug)]
pub enum ModelError {
    NotFound,
    AlreadyPresent,
}

pub type KeyType = String;
pub type InternalValue = Box<StoredValue>;

#[derive(Eq, PartialEq, Hash, Clone)]
pub struct StoredValue {
    data: String,
    ttl: Option<DateTime<Utc>>,
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelError::NotFound => write!(f, "Key Not Found"),
            ModelError::AlreadyPresent => write!(f, "Key is not present in the data set"),
        }
    }
}

impl Error for ModelError {}

pub struct Store {}

impl Store {
    pub fn insert(
        writer_m: &Mutex<WriteHandle<KeyType, InternalValue>>,
        key: KeyType,
        value: Value,
        ttl: Option<i64>,
    ) -> Result<(), ModelError> {
        let mut writer = writer_m.lock();
        let current_ttl = ttl.map(|ttl_val| Utc::now() + Duration::milliseconds(ttl_val));
        if writer.contains_key(&key) {
            return Err(ModelError::AlreadyPresent);
        } else {
            writer.insert(
                key,
                Box::new(StoredValue {
                    data: value.to_string(),
                    ttl: current_ttl,
                }),
            );
        }
        Ok(())
    }

    pub fn delete(writer_m: &Mutex<WriteHandle<KeyType, InternalValue>>, key: KeyType) -> Result<(), ModelError> {
        let mut writer = writer_m.lock();
        if !writer.contains_key(&key) {
            return Err(ModelError::NotFound);
        }
        writer.empty(key);
        Ok(())
    }

    pub fn get(reader: ReadHandle<KeyType, InternalValue>, key: KeyType) -> Result<Option<Value>, ModelError> {
        Ok(reader.get_one(&key).map(|v| json!(v.data.clone())))
    }

    pub async fn init() -> (
        ReadHandleFactory<KeyType, InternalValue>,
        Arc<Mutex<WriteHandle<KeyType, InternalValue>>>,
        JoinHandle<()>,
    ) {
        let (read_handle, mut write_handle): (ReadHandle<KeyType, InternalValue>, WriteHandle<KeyType, InternalValue>) =
            evmap::new();
        // initiall call used so that we can get accurate pending transactions
        // https://docs.rs/evmap/latest/evmap/struct.WriteHandle.html#method.pending
        write_handle.refresh();
        let writer = Arc::new(Mutex::new(write_handle));
        let internal_writer = writer.clone();
        let mut ttl_queue: DoublePriorityQueue<KeyType, DateTime<Utc>> = DoublePriorityQueue::new();
        let timer_handler = task::spawn(async move {
            loop {
                let mut write_handle = internal_writer.lock();
                for operation in write_handle.pending() {
                    match operation {
                        evmap::Operation::Add(k, v) => {
                            if let Some(ttl) = v.ttl {
                                ttl_queue.push(k.clone(), ttl);
                            }
                        },
                        evmap::Operation::Empty(k) => {
                            if ttl_queue.get(k).is_some() {
                                ttl_queue.remove(k);
                            }
                        },
                        _ => (),
                    }
                }
                let mut next_ttl = ttl_queue.peek_min();
                while next_ttl.is_some() && Utc::now() > *next_ttl.unwrap().1 {
                    write_handle.empty(next_ttl.unwrap().0.clone());
                    ttl_queue.pop_min();
                    next_ttl = ttl_queue.peek_min();
                }
                write_handle.refresh();
                #[cfg(test)]
                // wait for queue to clear for ttl testing
                if ttl_queue.is_empty() {
                    break;
                }
            }
        });
        (read_handle.factory(), writer, timer_handler)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use core::time::Duration;

    #[tokio::test]
    async fn insert() {
        let (_read_handle, write_handle, timer_handler) = Store::init().await;
        let k = "key".to_string();
        let v = json!("value");
        let _ = Store::insert(&write_handle, k.clone(), v, None);
        let _ = timer_handler.await;
        let _ = tokio::time::sleep(Duration::from_millis(1));
        let writer = write_handle.lock();
        assert!(writer.contains_key(&k));
    }

    #[tokio::test]
    async fn insert_with_ttl() {
        let (_read_handle, write_handle, timer_handler) = Store::init().await;
        let k = "key".to_string();
        let v = json!("value");
        let _ = Store::insert(&write_handle, k.clone(), v, Some(10));
        let _ = timer_handler.await;
        let _ = tokio::time::sleep(Duration::from_millis(10));
        let writer = write_handle.lock();
        assert!(!writer.contains_key(&k));
    }

    fn write_val(writer_m: &Mutex<WriteHandle<KeyType, InternalValue>>, k: String, v: Value) {
        let mut writer = writer_m.lock();
        writer.insert(
            k.clone(),
            Box::new(StoredValue {
                data: v.to_string(),
                ttl: None,
            }),
        );
        writer.refresh();
    }

    #[tokio::test]
    async fn delete() {
        let (_read_handle, write_handle, timer_handler) = Store::init().await;
        let k = "key".to_string();
        let v = json!("value");
        write_val(&write_handle, k.clone(), v);
        let _ = Store::delete(&write_handle, k.clone());
        let _ = timer_handler.await;
        let _ = tokio::time::sleep(Duration::from_millis(1));
        let writer = write_handle.lock();
        assert!(!writer.contains_key(&k));
    }

    #[tokio::test]
    async fn get() {
        let (read_handle, write_handle, _timer_handler) = Store::init().await;
        let k = "key".to_string();
        let v = json!("value");
        write_val(&write_handle, k.clone(), v.clone());
        assert_eq!(
            Some(json!("\"value\"")),
            Store::get(read_handle.handle(), k.clone()).unwrap()
        );
    }
}
