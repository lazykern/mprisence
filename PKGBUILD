# Maintainer: Phusit Somboonyingsuk

pkgname=mprisence
pkgver=0.4.1
pkgrel=1
pkgdesc="A Discord Rich Presence client for MPRIS-compatible media players with support for album art."
url="https://github.com/lazykern/mprisence"
license=("MIT")
arch=("x86_64")
provides=("mprisence")
conflicts=("mprisence")
source=("https://github.com/lazykern/mprisence/releases/download/v$pkgver/mprisence-$pkgver-x86_64.tar.gz" "https://raw.githubusercontent.com/lazykern/mprisence/main/LICENSE" "https://raw.githubusercontent.com/lazykern/mprisence/main/systemd/mprisence.service")
sha256sums=('4f442046192c8f1c27a40577cee173fbbc5e3b7a29ffda17c6ad2f16e673c8aa'
	'2efd06eb77e15171ec8727caece105c68fcf253f57d5af76d7964c69f16fbb7d'
	'821c33fb624e652c443c800f4003db4d0ac9eee6c49fabfe3987746afb39856a')

package() {
	install -Dm755 mprisence -t "$pkgdir/usr/bin"
	install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
	install -Dm644 mprisence.service "$pkgdir/usr/lib/systemd/user/mprisence.service"
}
