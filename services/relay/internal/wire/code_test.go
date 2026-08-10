package wire

import (
	"strings"
	"testing"
)

func TestNormalizeCode(t *testing.T) {
	tests := []struct {
		in   string
		want string
		why  string
	}{
		{"ABC123", "ABC123", "already canonical"},
		{"abc123", "ABC123", "typed in lower case"},
		{"  ABC123  ", "ABC123", "pasted with space around it"},
		{"ABC-123", "ABC123", "read aloud, written with a dash"},
		{"A B C 1 2 3", "ABC123", "spelled out one character at a time"},
		{"ABC_123", "ABC123", "underscore"},
		{"ABC.123", "ABC123", "full stop"},

		// The whole reason for Crockford's alphabet: these are what a person
		// types when they are looking at the other glyph.
		{"ABCO23", "ABC023", "O read as zero"},
		{"ABCI23", "ABC123", "I read as one"},
		{"ABCl23", "ABC123", "lower-case L read as one"},
		{"ABCL23", "ABC123", "L read as one"},

		{"", "", "nothing"},
		{"ABC12", "", "too short"},
		{"ABC1234", "", "too long"},
		{"ABC!23", "", "punctuation that is not a separator"},
		// U is not in the alphabet and is not a look-alike for anything that
		// is, so it can only be a typo — and guessing would send the viewer
		// into a room that is not theirs.
		{"ABCU23", "", "U"},
	}
	for _, tc := range tests {
		if got := NormalizeCode(tc.in); got != tc.want {
			t.Errorf("NormalizeCode(%q) = %q, want %q (%s)", tc.in, got, tc.want, tc.why)
		}
	}
}

func TestNormalizeIsIdempotent(t *testing.T) {
	// `ValidCode` is `NormalizeCode(s) == s`, so anything that normalises to a
	// code must normalise to itself the second time or a valid code would be
	// refused on arrival.
	for _, in := range []string{"abc-123", "ABCO23", "ABCl23", " zzz999 "} {
		once := NormalizeCode(in)
		if once == "" {
			t.Fatalf("NormalizeCode(%q) rejected a case this test needs", in)
		}
		if twice := NormalizeCode(once); twice != once {
			t.Errorf("NormalizeCode(%q) = %q, then %q — not idempotent", in, once, twice)
		}
		if !ValidCode(once) {
			t.Errorf("ValidCode(%q) is false for a code we just produced", once)
		}
	}
}

func TestGeneratedCodesAreValid(t *testing.T) {
	seen := map[string]bool{}
	for range 500 {
		code := NewCode()
		if len(code) != CodeLen {
			t.Fatalf("NewCode() = %q, want %d characters", code, CodeLen)
		}
		if !ValidCode(code) {
			t.Fatalf("NewCode() = %q, which does not survive NormalizeCode", code)
		}
		// The generator must not be able to emit the glyphs the alphabet
		// exists to avoid, or a code would be unreadable aloud.
		if strings.ContainsAny(code, "ILOU") {
			t.Fatalf("NewCode() = %q contains an ambiguous glyph", code)
		}
		seen[code] = true
	}
	// Not a randomness test — just proof that this is not a constant, which is
	// the failure a `rand` mistake actually produces.
	if len(seen) < 450 {
		t.Errorf("500 codes produced only %d distinct values", len(seen))
	}
}

func TestMemberIDsAreDistinctFromCodes(t *testing.T) {
	// Ids and codes are compared against each other nowhere, but they do share
	// an alphabet, and a member id that could be mistaken for a room code in a
	// log is a bad half-hour. The prefix is what keeps them apart.
	id := NewID()
	if !strings.HasPrefix(id, "m") || len(id) != IDLen+1 {
		t.Fatalf("NewID() = %q", id)
	}
}
