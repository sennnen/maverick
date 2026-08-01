# Bundled font licences

Old Standard TT is the only typeface Maverick ships inside the APK — the Terrain serif role, used
for headlines, verdicts, and every large numeral. It is licensed under the SIL Open Font License
1.1; the full text is in `OldStandardTT-OFL.txt`, and the font files are
`app/src/main/res/font/old_standard_tt_*.ttf`.

The sans role is Roboto, which the platform provides, so nothing is bundled for it. On iOS both
roles are Apple system faces — New York and SF Pro — reached through the font *design* rather than
by name, so nothing is bundled there either.

The licence text lives here rather than in `res/font/` because the resource merger rejects any file
in a font directory that is not `.xml`, `.ttf`, `.ttc` or `.otf`.
