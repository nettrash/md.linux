//! The typing switches: `TypingSettings` (md.win `SmartTypingAdapterTests.cs`, "The settings")
//! and `WrapSelectionSetting` (md.win `FormattingAdapterTests.cs`,
//! `WrapSelectionIsOnUntilItIsTurnedOff`). Ported with the settings module, so the typing hooks'
//! port can skip these five.

use std::cell::RefCell;
use std::rc::Rc;

use md_logic::settings::settings_keys::{self, ALL};
use md_logic::settings::{InMemorySettingsStore, SettingsStore, TypingSettings, wrap_selection_setting};
use md_logic::testing::FakeSettingsStore;

#[test]
fn both_typing_settings_default_to_on() {
    let store = InMemorySettingsStore::new();
    assert_eq!(TypingSettings::read(&store), TypingSettings::DEFAULTS);
    const { assert!(TypingSettings::DEFAULTS.continue_lists) };
    const { assert!(TypingSettings::DEFAULTS.capitalize_sentences) };
    assert_eq!(TypingSettings::default(), TypingSettings::DEFAULTS);
}

#[test]
fn the_settings_read_their_keys_and_nothing_else() {
    let store = InMemorySettingsStore::new();
    store.set_bool(settings_keys::CONTINUE_LISTS, false);
    assert_eq!(TypingSettings::read(&store), TypingSettings::new(false, true));
    store.set_bool(settings_keys::CAPITALIZE_SENTENCES, false);
    assert_eq!(TypingSettings::read(&store), TypingSettings::new(false, false));
    store.set_string(settings_keys::CONTINUE_LISTS, "true"); // the wrong type reads as absent
    assert_eq!(TypingSettings::read(&store), TypingSettings::new(true, false));
}

#[test]
fn toggle_flips_the_stored_value_from_its_default_and_raises_changed() {
    let store = InMemorySettingsStore::new();
    let changes = Rc::new(RefCell::new(Vec::<String>::new()));
    {
        let changes = changes.clone();
        store.changed().connect(move |key| changes.borrow_mut().push(key.to_string()));
    }

    TypingSettings::toggle(&store, settings_keys::CAPITALIZE_SENTENCES); // absent ⇒ true ⇒ off
    assert!(!store.get_bool(settings_keys::CAPITALIZE_SENTENCES, true));
    TypingSettings::toggle(&store, settings_keys::CAPITALIZE_SENTENCES);
    assert!(store.get_bool(settings_keys::CAPITALIZE_SENTENCES, false));
    TypingSettings::toggle(&store, settings_keys::CONTINUE_LISTS);
    assert!(!store.get_bool(settings_keys::CONTINUE_LISTS, true));

    assert_eq!(
        *changes.borrow(),
        [settings_keys::CAPITALIZE_SENTENCES, settings_keys::CAPITALIZE_SENTENCES, settings_keys::CONTINUE_LISTS]
    );
}

#[test]
#[should_panic(expected = "Not a typing setting")]
fn toggle_refuses_any_other_key() {
    // The last assertion of md.win's toggle test (ArgumentOutOfRangeException).
    TypingSettings::toggle(&InMemorySettingsStore::new(), settings_keys::PDF_PAGE_SIZE);
}

#[test]
fn only_the_two_keys_are_typing_keys() {
    assert!(TypingSettings::is_key(settings_keys::CONTINUE_LISTS));
    assert!(TypingSettings::is_key(settings_keys::CAPITALIZE_SENTENCES));
    for key in ALL.iter().filter(|k| **k != settings_keys::CONTINUE_LISTS && **k != settings_keys::CAPITALIZE_SENTENCES)
    {
        assert!(!TypingSettings::is_key(key), "{key}");
    }
}

#[test]
fn wrap_selection_is_on_until_it_is_turned_off() {
    let store = FakeSettingsStore::new();
    assert!(wrap_selection_setting::read(&store));
    wrap_selection_setting::toggle(&store);
    assert!(!wrap_selection_setting::read(&store));
    assert!(!store.get_bool(settings_keys::WRAP_SELECTION, true));
    wrap_selection_setting::toggle(&store);
    assert!(wrap_selection_setting::read(&store));
    assert!(wrap_selection_setting::is_key("md.wrapSelection"));
    assert!(!wrap_selection_setting::is_key(settings_keys::CONTINUE_LISTS));
    // Not one of smart typing's pair: the typing hooks never see it.
    assert!(!TypingSettings::is_key(settings_keys::WRAP_SELECTION));
}

#[test]
fn the_switches_read_through_a_trait_object() {
    // Rust only: the logic holds the store as `Rc<dyn SettingsStore>`.
    let store: Rc<dyn SettingsStore> = Rc::new(InMemorySettingsStore::new());
    store.set_bool(settings_keys::WRAP_SELECTION, false);
    assert!(!wrap_selection_setting::read(&*store));
    TypingSettings::toggle(&*store, settings_keys::CONTINUE_LISTS);
    assert_eq!(TypingSettings::read(&*store), TypingSettings::new(false, true));
}
