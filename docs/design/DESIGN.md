# Design-System

Verbindliche Regeln für alle Oberflächen von LLM Harness, also Web-App und Desktop-App. Ziel ist eine UI, die **ruhig, schnell und hochwertig** wirkt und dabei konsistent bleibt, egal wer oder was gerade Code schreibt.

> Quelle der Wahrheit für Werte ist [`apps/web/src/styles/tokens.css`](../../apps/web/src/styles/tokens.css). Motion-Werte gibt es zusätzlich für JavaScript in [`apps/web/src/lib/motion.ts`](../../apps/web/src/lib/motion.ts). Dieses Dokument erklärt, **wie** die Tokens zu verwenden sind.

## 1. Grundsätze

1. **Inhalt zuerst.** Der Chat ist das Produkt. Chrome (Leisten, Rahmen, Schatten) tritt zurück. Die meisten Flächen sind neutral, Farbe hat Bedeutung.
2. **Eine Akzentfarbe.** `primary` markiert die *eine* wichtigste Aktion im Blickfeld, z. B. Senden oder Speichern. Zwei primäre Buttons nebeneinander sind ein Fehler.
3. **Bewegung erklärt.** Animationen zeigen, woher etwas kommt und wohin es geht. Sie sind kurz, nie dekorativ, und `prefers-reduced-motion` wird immer respektiert.
4. **Tastatur ist gleichberechtigt.** Alles ist ohne Maus bedienbar, mit sichtbarem Fokus.
5. **Deutsch passt rein.** Deutsche Texte sind etwa 30 % länger als englische. Layouts werden mit deutschen Texten geprüft.

## 2. Token-Architektur

```
Primitive  (--palette-*)          nur in tokens.css, NIE in Komponenten
   │
Semantik   (--background, --primary, --elevation-1, --radius …)
   │
Tailwind   (@theme inline: bg-background, text-muted-foreground, shadow-elevation-1, rounded-lg …)
   │
Komponenten (src/components/ui → Primitive, src/features → Feature-Komponenten)
```

**Harte Regel:** In Komponenten stehen **keine festen Designwerte**: keine Farbe, keine Pixel für Abstände, Radien oder Schriftgrößen, keine Dauer, keine Kurve und kein Schatten. Verwendet werden ausschließlich Tailwind-Klassen, die auf Tokens zeigen, oder `var(--token)`.

- Die Standardpalette von Tailwind (`bg-zinc-900`, `text-blue-500`) ist **deaktiviert**. Solche Klassen erzeugen kein CSS.
- Fehlt ein Wert, wird **das Token ergänzt**, und zwar in allen Themes (hell und dunkel). Ein „nur dieses eine Mal“ inline gibt es nicht.
- Farben kommen immer **paarweise**: Jedes Hintergrund-Token hat ein passendes Vordergrund-Token (`primary` / `primary-foreground`). Die Vordergrundfarbe wird nie nach Augenmaß gewählt.

## 3. Farbe (semantisch)

| Token-Paar | Verwendung |
|---|---|
| `background` / `foreground` | Seitenhintergrund, Fließtext |
| `card` / `card-foreground` | Erhöhte Flächen: Karten, Composer, Dialoge |
| `popover` / `popover-foreground` | Menüs, Popover, Tooltips, Command-Palette |
| `primary` / `primary-foreground` | Die eine Hauptaktion, aktive Auswahl, Links im UI |
| `secondary` / `secondary-foreground` | Neutrale Buttons |
| `muted` / `muted-foreground` | Gedämpfte Flächen, Sekundärtext, Platzhalter |
| `accent` / `accent-foreground` | Hover- und aktive Zustände in Listen und Menüs |
| `destructive` / `destructive-foreground` | Löschen, Fehler |
| `destructive-subtle` | Hintergrund von Fehlermeldungen (Text: `destructive`) |
| `success` / `success-foreground` | Erfolg, „verbunden“ |
| `warning` / `warning-foreground` | Warnungen |
| `border`, `input`, `ring` | Trennlinien, Feldrahmen, Fokusring |
| `sidebar` / `sidebar-foreground`, `sidebar-accent` / `sidebar-accent-foreground`, `sidebar-border` | Seitenleiste |
| `bubble` / `bubble-foreground` | Nachrichten des Nutzers im Chat |
| `tooltip` / `tooltip-foreground` | Tooltips (bewusst kontrastreich, in beiden Modi dunkel) |
| `control-thumb` | Bewegliche Teile von Bedienelementen (Schalter-Daumen) |
| `overlay` | Abdunkelung hinter Dialogen |

Transparenzstufen sind über Tailwinds Opacity-Modifier erlaubt (`bg-primary/10`), weil sie das Token behalten.

**Kontrast:** Text mindestens 4,5:1, großer Text und Icons mindestens 3:1, **in beiden Modi**. `muted-foreground` ist so gewählt, dass es auf `background`, `muted` und `card` die 4,5:1 hält. Wer Text auf andere Flächen setzt, prüft nach.

**Bedeutung nie nur über Farbe:** Fehler haben ein Icon und Text, aktive Elemente zusätzlich Gewicht oder einen Indikator.

**Kompatibilitäts-Aliase:** Streamdown (Markdown-Rendering) nutzt intern einige Tailwind-Farben (`red-*`, `black`). Die sind in `globals.css` auf semantische Tokens umgebogen. Eigene Komponenten verwenden sie **nie**.

## 4. Die fünf Theme-Achsen

| Achse | Steuerung | Regel für Komponenten |
|---|---|---|
| **Palette** | `.dark` am `<html>`; später weitere Paletten über `[data-palette]` | Nur semantische Farb-Tokens |
| **Dichte** | `[data-density="compact" \| "comfortable"]` setzt `--space-unit` (Tailwind `--spacing`) | Abstände und Größen nur über die Spacing-Skala (`p-3`, `h-9`, `gap-2`), damit sie mitskalieren |
| **Trennung** | `--elevation-0…3` (Schatten, Haarlinie oder beides, je nach Modus) | Flächen trennen sich über `shadow-elevation-*`. Keine handgeschriebenen Schatten, Rahmen nur mit `border-border` |
| **Form** | `--radius` (Basis), daraus `rounded-sm…2xl` | Radien nur aus der Skala |
| **Bewegung** | `--duration-*`, `--easing-*`; Reduced Motion | Nur Motion-Tokens (CSS) bzw. `motion.ts` (JS) |

## 5. Elevation

| Token | Einsatz |
|---|---|
| `shadow-elevation-0` | Flach, keine Trennung (Standard) |
| `shadow-elevation-1` | Karten, Composer, Eingabefelder im Fokus |
| `shadow-elevation-2` | Popover, Menüs, Tooltips |
| `shadow-elevation-3` | Dialoge |

Im Hellmodus sind es weiche Schatten, im Dunkelmodus Haarlinie plus dunkler Schatten, weil Schatten auf dunklem Grund kaum sichtbar sind. Komponenten müssen in beiden Varianten gut aussehen.

## 6. Form und Abstand

- **Radius:** `--radius` = 0,75 rem. `rounded-sm` (Badges, Kbd), `rounded-md` (Buttons, Inputs), `rounded-lg` (Menüeinträge, kleine Karten), `rounded-xl` (Karten, Dialoge), `rounded-2xl` (Composer, Chat-Blasen), `rounded-full` (Avatare, runde Icon-Buttons).
- **Abstand:** Tailwind-Spacing-Skala auf Basis von `--space-unit`. Übliche Stufen: 1 = Mini-Abstand in Komponenten, 2–3 = Abstände in Komponenten, 4–6 = zwischen Komponenten, 8+ = zwischen Abschnitten.
- **Inhaltsbreite:** Chatspalte `max-w-(--content-width)`, Formulare `max-w-(--form-width)`.
- **Seitenleiste:** `w-(--sidebar-width)`.

## 7. Typografie

- **Schriften:** Geist (UI und Fließtext) und Geist Mono (Code, Tastenkürzel, IDs). Beide sind selbst gehostet (`@fontsource-variable`), es gibt keine externen Requests.
- **Skala:** Tailwinds `text-xs … text-3xl`. UI-Standard ist `text-sm`, Chattext `text-base`, Seitentitel `text-2xl font-semibold tracking-tight`.
- **Gewichte:** 400 (Text), 500 (Labels, Buttons), 600 (Überschriften). Kein 700+.
- **Zeilenlänge:** Fließtext höchstens etwa 75 Zeichen, deshalb die Chatbreite.

## 8. Bewegung

| Token | Wert | Einsatz |
|---|---|---|
| `--duration-instant` | 80 ms | Farbwechsel bei Hover/Press |
| `--duration-fast` | 150 ms | Ausblenden, kleine Elemente |
| `--duration-normal` | 220 ms | Einblenden von Menüs, Nachrichten |
| `--duration-slow` | 360 ms | Größere Layoutwechsel (Seitenleiste) |
| `--easing-standard` (Klasse `ease-standard`) | `cubic-bezier(0.2, 0, 0, 1)` | Zustandswechsel |
| `--easing-enter` (Klasse `ease-enter`) | `cubic-bezier(0, 0, 0, 1)` | Hereinkommen (abbremsend) |
| `--easing-exit` (Klasse `ease-exit`) | `cubic-bezier(0.3, 0, 1, 1)` | Herausgehen (beschleunigend) |

Regeln:

- **Asymmetrisch:** Ausgänge sind schneller als Eingänge (fast vs. normal).
- **Nur `transform` und `opacity` animieren**, keine Layout-Eigenschaften in Listen.
- **Federn** (`motion.ts` → `spring.*`) für direkte Manipulation und Layout-Animationen. Für Ein- und Ausblenden Tween mit Token-Dauer.
- **Reduced Motion:** `MotionConfig reducedMotion="user"` umschließt die App, und in CSS werden alle Dauern auf ≈0 gesetzt. Keine Einstellung *nur* in der App.
- **Keine Endlosschleifen**, außer bei echten Fortschrittsanzeigen (Tipp-Indikator, Spinner).

## 9. Komponenten

**Primitive** liegen in `apps/web/src/components/ui/`, sind domänenfrei und werden über ihre Dateien importiert. **Feature-Komponenten** liegen in `apps/web/src/features/<feature>/`, bauen **nur** auf Primitiven auf und greifen nie direkt auf Radix, cmdk o. ä. zu.

Vor einer neuen Komponente steht die Frage, ob eine **Variante** einer bestehenden reicht. Zwei ähnlich aussehende Komponenten driften auseinander.

| Primitiv | Varianten |
|---|---|
| `Button` | `variant`: primary, secondary, ghost, outline, destructive, link · `size`: sm, md, lg, icon, icon-sm |
| `Input`, `Textarea`, `Label`, `Field` | Fehlerzustand über `aria-invalid` |
| `Dialog`, `AlertDialog` | — |
| `DropdownMenu`, `Popover`, `Tooltip`, `Command` | — |
| `Tabs`, `Switch`, `Badge` (`variant`: neutral, primary, success, warning, destructive), `Avatar`, `Skeleton`, `Kbd`, `Spinner`, `Card` | — |
| `Toaster` (sonner) | — |

### Zustände

Jede interaktive Komponente setzt die passenden Zustände um und prüft sie sichtbar: `default`, `hover`, `pressed`, `focus-visible`, `disabled`, `loading`, `error`, `selected`, `empty`.

- **Fokus:** `focus-visible:` mit dem Ring-Token (`--focus-ring-width`, `ring-ring`), in jedem Modus sichtbar.
- **Disabled:** reduzierte Deckkraft **und** `cursor-not-allowed`, nicht nur eine andere Farbe.
- **Loading:** Skeleton in der Form des Inhalts, kein Spinner anstelle des ganzen Layouts. Das Layout springt nicht, wenn Daten kommen.
- **Leer:** jede Liste hat einen Leerzustand mit Erklärung und nächster Aktion.

## 10. Layout

- **Desktop:** Seitenleiste links (`--sidebar-width`), Inhalt rechts. Der Chat ist zentriert.
- **Mobil (< `md`):** Die Seitenleiste wird zum Drawer, die Kopfzeile bekommt einen Menü-Button. Es gibt **nie** horizontales Scrollen der Seite. Breite Inhalte (Code, Tabellen) scrollen in ihrem eigenen Container.
- **Touch:** Trefferfläche mindestens 44 × 44 px, auch wenn das Icon kleiner ist (`size="icon"` erfüllt das auf Touch-Geräten über `pointer-coarse:`).

## 11. Sprache und Ton

- UI-Texte stehen **nie inline** im Code, sondern in `apps/web/src/i18n/` (Deutsch ist die Quelle, Englisch die Übersetzung).
- Deutsch mit **„du“**, kurz und freundlich, ohne Ausrufezeichen. Satzanfang groß, sonst normale Groß- und Kleinschreibung („Neuer Chat“, nicht „Neuer CHAT“).
- Buttons sagen, was passiert („Key speichern“ statt „OK“).
- Fehlermeldungen nennen Ursache und nächsten Schritt („Der Anbieter hat den Key abgelehnt. Prüfe ihn in den Einstellungen.“).
- Markennamen (Anthropic, OpenAI …) werden nicht übersetzt.

## 12. Barrierefreiheit

- Jeder Button, der nur ein Icon zeigt, hat ein `aria-label` (über i18n).
- Semantisches HTML zuerst (`button`, `nav`, `main`, `h1…h3`, Listen).
- Dialoge fangen den Fokus und geben ihn beim Schließen zurück (Radix).
- Live-Regionen für Streaming-Antworten: Die Nachrichtenliste ist `aria-live="polite"`.

## 13. Prüfen: die Design-Seite

Unter **`/design`** (nur im Entwicklungsmodus) zeigt die App alle Primitive mit allen Varianten und Zuständen, dazu Schalter für Modus (hell/dunkel) und Dichte. Bevor eine Komponente als fertig gilt:

1. Eintrag auf der Design-Seite anlegen bzw. erweitern.
2. In **hell und dunkel** und in **allen drei Dichten** ansehen.
3. Fokus per Tastatur durchgehen.
4. Mit `prefers-reduced-motion` prüfen (DevTools → Rendering).
5. Schmale Breite (≈ 360 px) prüfen.

Im Abschlussbericht steht, was davon tatsächlich geprüft wurde.

## 14. Do & Don't

| Don't | Do |
|---|---|
| `className="bg-zinc-900 text-white"` | `className="bg-primary text-primary-foreground"` |
| `style={{ padding: 13 }}` | `className="p-3"` |
| `className="shadow-lg"` | `className="shadow-elevation-2"` |
| `transition={{ duration: 0.3 }}` | `transition={transitions.enter}` aus `motion.ts` |
| `<button><X /></button>` | `<Button size="icon" aria-label={t('common.close')}><X /></Button>` |
| Text im JSX: `"Speichern"` | `t('common.save')` |
| Neue Komponente „FancyButton“ | Neue `variant` am `Button` |
