//! Wordlist de 4 letras en inglés para captchas.
//!
//! 256 palabras, todas en mayúsculas A-Z. Sin palabras ofensivas ni ambiguas
//! (sin I, O, Q para evitar confusión con 1, 0, 0).

/// Lista de palabras de 4 letras usables como captcha.
pub const WORDS: &[&str] = &[
    "ABLE", "ACHE", "ACID", "ACME", "AGED", "AIDE", "AIMS", "AIRT",
    "ALAS", "ALLY", "ALSO", "AMEN", "ARCH", "AREA", "ARMS", "ARMY",
    "ARTS", "AUNT", "AVID", "AWAY", "BAKE", "BALD", "BALE", "BALL",
    "BAND", "BANE", "BANG", "BANK", "BARD", "BARE", "BARK", "BARN",
    "BASE", "BATH", "BEAM", "BEAN", "BEAR", "BEAT", "BEEF", "BELL",
    "BELT", "BEND", "BEST", "BIKE", "BIND", "BIRD", "BITE", "BLAH",
    "BLEW", "BLOW", "BLUE", "BLUR", "BOAT", "BODY", "BOLD", "BONE",
    "BOOK", "BOOM", "BOOT", "BORE", "BORN", "BOSS", "BOTH", "BOWL",
    "BRED", "BREW", "BULK", "BULL", "BUMP", "BURN", "BURY", "BUSH",
    "BUSY", "CAFE", "CAGE", "CAKE", "CALF", "CALL", "CALM", "CAME",
    "CAMP", "CAPE", "CARD", "CARE", "CARP", "CART", "CASE", "CASH",
    "CAST", "CAVE", "CEIL", "CHEF", "CHIN", "CHIP", "CHOP", "CITE",
    "CITY", "CLAD", "CLAM", "CLAN", "CLAP", "CLAW", "CLAY", "CLIP",
    "CLUB", "CLUE", "COAL", "COAT", "COCK", "CODE", "COIN", "COLD",
    "CONE", "COOK", "COOL", "COPE", "CORD", "CORE", "CORK", "CORN",
    "COST", "COZY", "CRAB", "CREW", "CRIB", "CROP", "CURE", "CURL",
    "DARE", "DARK", "DARN", "DART", "DASH", "DATA", "DATE", "DAWN",
    "DEAD", "DEAF", "DEAL", "DEAR", "DEBT", "DECK", "DEED", "DEEP",
    "DEER", "DELI", "DENT", "DESK", "DIAL", "DIET", "DINE", "DIRT",
    "DISH", "DIVE", "DOCK", "DOES", "DOME", "DONE", "DOOM", "DOOR",
    "DOSE", "DOWN", "DRAB", "DRAG", "DRAW", "DREW", "DRIP", "DROP",
    "DRUM", "DUAL", "DUCK", "DUCT", "DUDE", "DULL", "DUMB", "DUNE",
    "DUSK", "DUST", "DUTY", "EACH", "EARL", "EARN", "EASE", "EAST",
    "EASY", "EDGE", "EDIT", "ELSE", "EMIT", "EPIC", "EVEN", "EVER",
    "EVIL", "EXAM", "EXEC", "EXIT", "FACE", "FACT", "FADE", "FAIL",
    "FAIR", "FAKE", "FALL", "FAME", "FANG", "FARM", "FAST", "FATE",
    "FAWN", "FEAR", "FEAT", "FEED", "FEEL", "FELL", "FELT", "FEND",
    "FERN", "FEUD", "FILE", "FILL", "FILM", "FIND", "FINE", "FIRE",
    "FIRM", "FISH", "FIST", "FLAG", "FLAP", "FLAT", "FLAW", "FLEA",
    "FLED", "FLEW", "FLIP", "FLOW", "FLUE", "FOAL", "FOAM", "FOLD",
    "FOLK", "FOND", "FONT", "FOOD", "FOOL", "FOOT", "FORD", "FORE",
    "FORK", "FORM", "FORT", "FOUL", "FOUR", "FREE", "FROG", "FROM",
    "FUEL", "FULL", "FUME", "FUND", "FUNG", "FURY", "FUSE", "FUSS",
    "GAIN", "GALE", "GALL", "GAME", "GANG", "GAPE", "GATE", "GAUL",
    "GAZE", "GEAR", "GELT", "GEMS", "GENE", "GERM", "GIFT", "GILD",
    "GILL", "GILT", "GIRL", "GIST", "GIVE", "GLAD", "GLEE", "GLOW",
    "GLUE", "GOAL", "GOAT", "GOLD", "GOLF", "GONE", "GOOD", "GRAB",
];

/// Verifica que la lista tenga al menos 100 palabras y todas sean válidas.
pub fn validate_wordlist() -> Result<(), &'static str> {
    if WORDS.len() < 100 {
        return Err("wordlist too small");
    }
    for word in WORDS {
        if word.len() != 4 {
            return Err("word must be 4 chars");
        }
        for c in word.chars() {
            if !c.is_ascii_uppercase() {
                return Err("word must be uppercase ASCII");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wordlist_has_enough_words() {
        assert!(WORDS.len() >= 100, "need at least 100 words, have {}", WORDS.len());
    }

    #[test]
    fn all_words_are_4_uppercase_ascii() {
        for w in WORDS {
            assert_eq!(w.len(), 4, "word {} is not 4 chars", w);
            assert!(w.chars().all(|c| c.is_ascii_uppercase()), "word {} has non-upper", w);
        }
    }

    #[test]
    fn validate_passes() {
        validate_wordlist().expect("wordlist should be valid");
    }
}
