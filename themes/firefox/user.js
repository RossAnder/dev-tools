// eggshell for Firefox: prefs that userChrome.css cannot set. Firefox reads
// only the profile's own user.js, so append these lines to it.
//
// A new window paints the -moz-dialog system colour until the browser chrome's
// styles load. On GTK that colour comes from the GTK 3 theme (a mid grey under
// most dark themes), so it flashes before the chrome appears. These set it to
// the recessed chrome surface, scheme by scheme; other GTK apps are unaffected.
user_pref("ui.-moz-dialog", "#e7e4e0");
user_pref("ui.-moz-dialog.dark", "#000000");
user_pref("ui.-moz-dialogtext", "#171717");
user_pref("ui.-moz-dialogtext.dark", "#f2f2f2");
