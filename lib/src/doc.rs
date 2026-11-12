use crate::array::YrsArray;
use crate::error::CodingError;
use crate::map::YrsMap;
use crate::text::YrsText;
use crate::transaction::YrsTransaction;
use std::sync::Arc;
use std::{borrow::Borrow, cell::RefCell};
use yrs::{updates::decoder::Decode, ArrayRef, Doc, OffsetKind, Options, StateVector, Transact, Origin};
use yrs::{MapRef, ReadTxn};
use yrs::branch::Branch;
use crate::undo::YrsUndoManager;
use crate::subscription::YSubscription;
use std::fmt::Debug;

pub(crate) trait YrsUpdateObserver: Send + Sync + Debug {
    fn call(&self, update: Vec<u8>, origin: Option<YrsOrigin>);
}

/// Distinct client ids (ascending) of the blocks inserted by a v1 update.
pub(crate) fn update_v1_client_ids(update: Vec<u8>) -> Result<Vec<u64>, CodingError> {
    let u = yrs::Update::decode_v1(&update).map_err(|_| CodingError::DecodingError)?;
    let mut ids: Vec<u64> = u.insertions(true).iter().map(|(c, _)| c.get()).collect();
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}
use crate::UniffiCustomTypeConverter;

pub(crate) struct YrsDoc(RefCell<Doc>);

unsafe impl Send for YrsDoc {}
unsafe impl Sync for YrsDoc {}

impl YrsDoc {
    pub(crate) fn new() -> Self {
        let mut options = Options::default();
        options.offset_kind = OffsetKind::Utf16;
        let doc = yrs::Doc::with_options(options);

        Self(RefCell::from(doc))
    }

    pub(crate) fn new_with_client_id(client_id: u64) -> Self {
        let mut options = Options::default();
        options.offset_kind = OffsetKind::Utf16;
        options.client_id = yrs::block::ClientID::new(client_id);
        Self(RefCell::from(yrs::Doc::with_options(options)))
    }

    pub(crate) fn client_id(&self) -> u64 {
        self.0.borrow().client_id().get()
    }

    /// Subscribes to local-update notifications: `delegate.call(update, origin)` runs
    /// after each transaction commits (when the transaction is freed/dropped), with the
    /// origin given to `transact`. Applied remote updates fire it too.
    ///
    /// The callback runs synchronously inside the commit while yrs holds the store, and
    /// this binding's `YrsTransaction::free` has already released its RefCell borrow, so
    /// the callback must NOT open a transaction on the same doc (it would panic/deadlock).
    /// Hand the bytes off (e.g. to a queue) and return.
    pub(crate) fn observe_update_v1(
        &self,
        delegate: Box<dyn YrsUpdateObserver>,
    ) -> Result<Arc<YSubscription>, CodingError> {
        let sub = self
            .0
            .borrow()
            .observe_update_v1(move |txn, event| {
                let origin = txn.origin().cloned().map(YrsOrigin::from);
                delegate.call(event.update.clone(), origin);
            })
            .map_err(|_e| CodingError::EncodingError)?;
        Ok(Arc::new(YSubscription::new(sub)))
    }

    pub(crate) fn encode_diff_v1(
        &self,
        transaction: &YrsTransaction,
        state_vector: Vec<u8>,
    ) -> Result<Vec<u8>, CodingError> {
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();

        StateVector::decode_v1(state_vector.borrow())
            .map_err(|_e| CodingError::DecodingError)
            .map(|sv| tx.encode_diff_v1(&sv))
    }

    pub(crate) fn get_text(&self, name: String) -> Arc<YrsText> {
        let text_ref = self.0.borrow().get_or_insert_text(name.as_str());
        Arc::from(YrsText::from(text_ref))
    }

    pub(crate) fn get_array(&self, name: String) -> Arc<YrsArray> {
        let array_ref: ArrayRef = self.0.borrow().get_or_insert_array(name.as_str()).into();
        Arc::from(YrsArray::from(array_ref))
    }

    pub(crate) fn get_map(&self, name: String) -> Arc<YrsMap> {
        let map_ref: MapRef = self.0.borrow().get_or_insert_map(name.as_str()).into();
        Arc::from(YrsMap::from(map_ref))
    }

    pub(crate) fn transact<'doc>(&self, origin: Option<YrsOrigin>) -> Arc<YrsTransaction> {
        let tx = self.0.borrow();
        let tx = if let Some(origin) = origin {
            tx.transact_mut_with(origin)
        } else {
            tx.transact_mut()
        };
        Arc::from(YrsTransaction::from(tx))
    }

    pub(crate) fn undo_manager(&self, tracked_refs: Vec<YrsCollectionPtr>) -> Arc<YrsUndoManager> {
        let doc = &*self.0.borrow();
        let mut i = tracked_refs.into_iter();
        let first = i.next().unwrap();
        let mut undo_manager = yrs::undo::UndoManager::new(doc, &first);
        while let Some(n) = i.next() {
            undo_manager.expand_scope(&n);
        }
        Arc::new(YrsUndoManager::from(undo_manager))
    }
}

#[derive(Clone)]
pub(crate) struct YrsOrigin(Arc<[u8]>);

impl From<Origin> for YrsOrigin {
    fn from(value: Origin) -> Self {
        YrsOrigin(Arc::from(value.as_ref()))
    }
}

impl Into<Origin> for YrsOrigin {
    fn into(self) -> Origin {
        Origin::from(self.0.as_ref())
    }
}

impl UniffiCustomTypeConverter for YrsOrigin {
    type Builtin = Vec<u8>;

    fn into_custom(val: Self::Builtin) -> uniffi::Result<Self> where Self: Sized {
        Ok(YrsOrigin(val.into()))
    }

    fn from_custom(obj: Self) -> Self::Builtin {
        obj.0.to_vec()
    }
}

#[derive(Copy, Clone)]
#[repr(transparent)]
pub(crate) struct YrsCollectionPtr(*const Branch);

unsafe impl Send for YrsCollectionPtr { }
unsafe impl Sync for YrsCollectionPtr { }

impl AsRef<Branch> for YrsCollectionPtr {
    #[inline]
    fn as_ref(&self) -> &Branch {
        unsafe { self.0.as_ref() }.unwrap()
    }
}

impl<'a> From<&'a Branch> for YrsCollectionPtr {
    #[inline]
    fn from(value: &'a Branch) -> Self {
        let ptr = value as *const Branch;
        YrsCollectionPtr(ptr)
    }
}

impl UniffiCustomTypeConverter for YrsCollectionPtr {
    type Builtin = u64;

    fn into_custom(val: Self::Builtin) -> uniffi::Result<Self> where Self: Sized {
        let ptr = val as usize as *const Branch;
        Ok(YrsCollectionPtr(ptr))
    }

    fn from_custom(obj: Self) -> Self::Builtin {
        obj.0 as usize as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct Rec(Mutex<Vec<(Vec<u8>, Option<Vec<u8>>)>>);
    #[derive(Debug)]
    struct Obs(Arc<Rec>);
    impl YrsUpdateObserver for Obs {
        fn call(&self, update: Vec<u8>, origin: Option<YrsOrigin>) {
            self.0 .0.lock().unwrap().push((update, origin.map(YrsOrigin::from_custom_vec)));
        }
    }
    impl YrsOrigin {
        fn from_custom_vec(o: YrsOrigin) -> Vec<u8> { o.0.to_vec() }
    }

    #[test]
    fn client_id_round_trip() {
        let doc = YrsDoc::new_with_client_id(1234);
        assert_eq!(doc.client_id(), 1234);
        let m = doc.get_map("m".to_string());
        let txn = doc.transact(None);
        m.insert_string(&txn, "k".to_string(), "v".to_string());
        let upd = txn.transaction_encode_state_as_update();
        txn.free();
        assert_eq!(update_v1_client_ids(upd).unwrap(), vec![1234]);
        assert!(update_v1_client_ids(vec![0xff, 0xff, 0xff]).is_err());
    }

    #[test]
    fn observe_update_fires_once_per_commit_with_origin() {
        let a = YrsDoc::new_with_client_id(1);
        let rec = Arc::new(Rec::default());
        let _sub = a.observe_update_v1(Box::new(Obs(rec.clone()))).unwrap();
        let m = a.get_map("m".to_string());

        let txn = a.transact(Some(YrsOrigin(Arc::from(&b"local"[..]))));
        m.insert_string(&txn, "x".to_string(), "1".to_string());
        m.insert_string(&txn, "y".to_string(), "2".to_string());
        assert_eq!(rec.0.lock().unwrap().len(), 0, "not fired before commit");
        let upd = txn.transaction_encode_update();
        txn.free();
        {
            let got = rec.0.lock().unwrap();
            assert_eq!(got.len(), 1);
            assert_eq!(got[0].1.as_deref(), Some(&b"local"[..]));
            assert_eq!(update_v1_client_ids(got[0].0.clone()).unwrap(), vec![1]);
        }

        // remote apply into a second doc with a distinct origin
        let b = YrsDoc::new_with_client_id(2);
        let rec_b = Arc::new(Rec::default());
        let _sub_b = b.observe_update_v1(Box::new(Obs(rec_b.clone()))).unwrap();
        let t = b.transact(Some(YrsOrigin(Arc::from(&b"remote"[..]))));
        t.transaction_apply_update(upd).unwrap();
        t.free();
        let got = rec_b.0.lock().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1.as_deref(), Some(&b"remote"[..]));
        assert_eq!(update_v1_client_ids(got[0].0.clone()).unwrap(), vec![1]);
    }

    #[test]
    fn observe_fires_on_drop_without_free() {
        let a = YrsDoc::new();
        let rec = Arc::new(Rec::default());
        let _sub = a.observe_update_v1(Box::new(Obs(rec.clone()))).unwrap();
        let m = a.get_map("m".to_string());
        {
            let txn = a.transact(None);
            m.insert_string(&txn, "x".to_string(), "1".to_string());
        }
        assert_eq!(rec.0.lock().unwrap().len(), 1);
    }
}
