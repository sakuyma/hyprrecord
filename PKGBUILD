# Maintainer: sakuyma <sakuyma@proton.me>
# Contributor: sakuyma <sakuyma@proton.me>

pkgname=hyprscreen
pkgver=1.0.0
pkgrel=1
pkgdesc="Screenshot and recording tool for Hyprland"
arch=('x86_64')
url="https://github.com/sakuyma/hyprscreen"
license=('MIT')
depends=(
    'gtk4'
    'libadwaita'
    'slurp'
    'grim'
    'wf-recorder'
    'ffmpeg'
)
makedepends=('cargo' 'git')
source=("$pkgname-$pkgver.tar.gz::https://github.com/sakuyma/$pkgname/archive/v$pkgver.tar.gz")
sha256sums=('SKIP')

build() {
    cd "$srcdir/$pkgname-$pkgver"
    cargo build --release
}

package() {
    cd "$srcdir/$pkgname-$pkgver"
    install -Dm755 "target/release/$pkgname" "$pkgdir/usr/bin/$pkgname"
}
