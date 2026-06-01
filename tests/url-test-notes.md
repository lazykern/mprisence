# URL test notes

## Tested
- [x] youtube music (music.youtube.com)
- [x] soundcloud (soundcloud.com, snd.sc)
- [x] youtube (youtube.com, youtu.be)
- [x] bandcamp (bandcamp.com)
- [x] apple music (music.apple.com)

## To test
- [x] deezer
- [x] pocket casts
- [x] podurama
- [x] apple podcasts

## Skipped (need sub)
- [ ] qobuz, amazon music, yandex music
- [x] tidal (tested, works)

## Findings
- **bandcamp**: needs special integration (no standard MPRIS metadata iframe)
- **youtube music**: song length keeps incrementing on playlists (duration drift on each track change)
- **deezer**: MPRIS artwork is wrong (mismatched / stale images)
- **pocket casts**: needs better special integration
- **apple podcasts**: needs integration, partially works
