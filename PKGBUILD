# Maintainer: Phusit Somboonyingsuk

pkgname=mprisence
pkgver=0.5.2
pkgrel=1
pkgdesc="A Discord Rich Presence client for MPRIS-compatible media players with support for album art."
url="https://github.com/lazykern/mprisence"
license=("MIT")
arch=("x86_64")
provides=("mprisence")
conflicts=("mprisence")
source=("https://github.com/lazykern/mprisence/releases/download/v$pkgver/mprisence-$pkgver-x86_64.tar.gz" "https://raw.githubusercontent.com/lazykern/mprisence/main/LICENSE" "https://raw.githubusercontent.com/lazykern/mprisence/main/systemd/mprisence.service")
sha256sums=('b45ffab6778429bddb44e5ecfe40fb5db22d8f83ff43c40321ae29252c2f60be'
	'2efd06eb77e15171ec8727caece105c68fcf253f57d5af76d7964c69f16fbb7d'
	'6b69bd6bf3c0ef8a7e1c5ddfd537992d81156af0d59428d2e4aa399a9adc5dd4')

package() {
	install -Dm755 mprisence -t "$pkgdir/usr/bin"
	install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
	install -Dm644 mprisence.service "$pkgdir/usr/lib/systemd/user/mprisence.service"
}
