use super::*;

struct MockTranslation;
impl TranslationApi for MockTranslation {
    fn resolve(
        &self,
        key: Key<'_>,
        mut buffer: slice::Mut<'_, u8>,
        required: &mut usize,
    ) -> api::Status {
        let text = match (key.kind(), key.record()) {
            (KeyKind::Stage, 0) => {
                *required = 0;
                return api::NOT_FOUND;
            }
            (KeyKind::Stage, 1) => String::new(),
            _ => format!("中文:{key}"),
        };
        *required = text.len();
        if buffer.len() < text.len() {
            return api::BUFFER_TOO_SMALL;
        }
        buffer[..text.len()].copy_from_slice(text.as_bytes());
        api::OK
    }
}

#[test]
fn uses_borrowed_resource_keys_and_preserves_missing_vs_empty() {
    let table: TranslationTable = Box::new(MockTranslation).into();
    let translation = unsafe { bind(&table) };
    let resource = String::from("mhfdat");
    let group = String::from("quest");
    let key = Key::resource(&resource, &group, 42, 3);
    assert_eq!(key.resource_id().as_ptr(), resource.as_ptr());
    assert_eq!(key.group_id().as_ptr(), group.as_ptr());
    assert_eq!(
        translation.resolve(key).unwrap().as_deref(),
        Some("中文:mhfdat:quest:42:03")
    );
    assert_eq!(translation.resolve(Key::stage(1, 23, 0)).unwrap(), None);
    assert_eq!(
        translation.resolve(Key::stage(1, 23, 1)).unwrap(),
        Some(String::new())
    );
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| translation.resolve(key).unwrap()))
            .collect();
        for worker in workers {
            assert_eq!(
                worker.join().unwrap().as_deref(),
                Some("中文:mhfdat:quest:42:03")
            );
        }
    });
}
