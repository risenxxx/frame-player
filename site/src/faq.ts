/**
 * The home page's questions — the ones people ask an assistant before they ask
 * a search box. The questions themselves are in the dictionary (`faq.items`
 * in i18n/en.ts), one list per language. The same list is rendered by
 * `components/Faq.astro` and published as `FAQPage` structured data by the
 * home page, so the two cannot disagree.
 */
import { strings, type Locale, type Strings } from './i18n/index.ts'

export type Question = Strings['faq']['items'][number]

export const faqFor = (lang: Locale): Question[] => strings(lang).faq.items
