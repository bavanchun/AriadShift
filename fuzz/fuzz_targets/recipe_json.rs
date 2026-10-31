#![no_main]

use ariad_core::tools::{Recipe, RecipeOrigin, validate_naming_template};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = validate_naming_template(s);
        if let Ok(recipe) = Recipe::from_json_str(s, RecipeOrigin::Trusted) {
            let _ = recipe.validate(RecipeOrigin::Imported);
            let serialized = serde_json::to_string(&recipe).expect("validated Recipe serializes");
            let roundtrip = Recipe::from_json_str(&serialized, RecipeOrigin::Trusted)
                .expect("serialized Recipe round-trips");
            assert_eq!(roundtrip, recipe);
        }
        let _ = Recipe::from_json_str(s, RecipeOrigin::Imported);
    }
});
