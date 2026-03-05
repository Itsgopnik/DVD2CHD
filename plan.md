# Plan: CHD-Animation ersetzen (Conveyor Belt + Roller)

## Zusammenfassung
Die bestehende `draw_compression_disc` Methode in `animation.rs` (Hydraulikpresse) wird durch die neue Conveyor-Belt + Shredder-Roller Animation aus der JSX-Vorlage ersetzt.

## Was sich ändert
**Nur eine Methode:** `draw_compression_disc()` in `dvd2chd-gui/src/app/animation.rs` (Zeilen 440-672)

Alles andere (PhaseAnim, AnimationState, update(), andere Animationen) bleibt unverändert.

## Neue Animation — Elemente aus der JSX-Vorlage

1. **Förderbänder (Input + Output)** — Zwei horizontale Bänder links und rechts der Roller, mit animierten Tick-Markierungen und Endrollen
2. **Shredder-Roller (2x)** — Obere und untere rotierende Walze mit Zähnen/Grip-Markierungen, Gehäuse-Rahmen, Mittelbolzen
3. **Pakete** — Gleichmäßig verteilte Pakete die über das Band wandern:
   - Vor den Rollern: Groß, orange (RAW)
   - In den Rollern: Verformung (breiter + flacher), Farbwechsel orange→rot→grün
   - Nach den Rollern: Klein, grün (CHD), mit Häkchen
4. **Friktions-Glow** — Leuchteffekt zwischen den Rollern wenn aktiv
5. **Funken-Partikel** — Kleine Partikel beim Quetschen (vereinfacht für egui, da kein `Math.random()` pro Frame)
6. **Labels** — "INPUT", "ROLLERS", "OUTPUT" Beschriftungen
7. **Größenvergleich** — Gestrichelte Umrisse für RAW vs CHD Größe

## Umsetzungsstrategie (JSX → egui)

| JSX-Konzept | egui-Entsprechung |
|---|---|
| `ctx.fillRect` | `painter.rect_filled()` |
| `ctx.strokeRect` | `painter.rect()` mit Stroke |
| `ctx.arc` (Kreis) | `painter.circle_filled()` / `circle_stroke()` |
| `ctx.moveTo/lineTo` | `painter.line_segment()` |
| `ctx.fillText` | `painter.text()` |
| `rgba(r,g,b,a)` | `Color32::from_rgba_unmultiplied()` oder `linear_multiply()` |
| `ctx.setLineDash` | `painter.add(Shape::dashed_line(...))` |
| `requestAnimationFrame` | Bereits vorhanden via `ctx.request_repaint_after()` |
| `performance.now()` | `self.compress.phase_smooth` (0.0–1.0 Zyklus) |
| `Math.random()` für Partikel | Deterministische Pseudo-Random basierend auf `phase_smooth` + Index |

## Implementierungsschritte

1. **Die bestehende `draw_compression_disc` Methode komplett ersetzen** (Zeilen 440-672)
2. Neue Methode verwendet weiterhin:
   - `self.compress.phase_smooth` als Zeitbasis (statt `performance.now()`)
   - `self.compress.drive` als Aktivitäts-Faktor (0.0 = idle, 1.0 = aktiv)
   - `accent` Farbe aus dem Theme-System (statt hardcodierter Farben)
   - `visuals.extreme_bg_color` als Hintergrund
3. Farben werden theme-aware: Orange/Grün-Töne werden relativ zum `accent` berechnet
4. Keine neuen Felder in `AnimationState` nötig — `compress.phase_smooth` + `compress.drive` reichen
