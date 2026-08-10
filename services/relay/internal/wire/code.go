// Room codes, and the one property that matters about them: a person reads one
// out loud and another person types it in.
//
// Crockford's base32 alphabet, which drops the four glyphs that make that go
// wrong — I, L, O and U — and, crucially, *normalises* the ones people type
// anyway: `I`, `i`, `l` and `L` are read as `1`, and `O`/`o` as `0`. An
// alphabet that merely excluded the look-alikes would refuse the code the
// viewer is looking at rather than accept it, which is the worse half of the
// same problem. U is out to keep an accident from spelling something.
//
// 32^6 ≈ 1.07e9, which is not a secret by cryptographic standards and is not
// meant to be one: the code is a handle, its lifetime is an evening, and the
// relay rate-limits joins per address so it cannot be walked.
package wire

import (
	"crypto/rand"
	"math/big"
	"strings"
)

const codeAlphabet = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"

// CodeLen is what the UI promises: six characters.
const CodeLen = 6

// IDLen is a member id — never shown, only compared, so it is longer.
const IDLen = 10

func randomString(n int) string {
	max := big.NewInt(int64(len(codeAlphabet)))
	out := make([]byte, n)
	for i := range out {
		// crypto/rand, not math/rand: a predictable code is a room anyone can
		// walk into, and the cost here is a few microseconds an evening.
		v, err := rand.Int(rand.Reader, max)
		if err != nil {
			// rand.Reader failing is not survivable and not something to paper
			// over with a weaker source.
			panic("relay: no entropy: " + err.Error())
		}
		out[i] = codeAlphabet[v.Int64()]
	}
	return string(out)
}

func NewCode() string { return randomString(CodeLen) }
func NewID() string   { return "m" + randomString(IDLen) }

// NormalizeCode turns what somebody typed into what the relay stores, or ""
// when it cannot be a code at all.
//
// Forgiving on purpose: case, surrounding space, and the separators people add
// when they read a code back to themselves ("ABC-123") are all discarded, and
// the four ambiguous glyphs are folded onto their look-alikes.
func NormalizeCode(s string) string {
	var b strings.Builder
	for _, r := range strings.ToUpper(strings.TrimSpace(s)) {
		switch r {
		case ' ', '-', '_', '.':
			continue
		case 'I', 'L':
			r = '1'
		case 'O':
			r = '0'
		case 'U':
			// Not in the alphabet and not a look-alike for anything in it —
			// so this is a typo, and accepting it as something else would send
			// the viewer into a room that is not theirs.
			return ""
		}
		if !strings.ContainsRune(codeAlphabet, r) {
			return ""
		}
		b.WriteRune(r)
	}
	if b.Len() != CodeLen {
		return ""
	}
	return b.String()
}

func ValidCode(s string) bool { return NormalizeCode(s) == s }
