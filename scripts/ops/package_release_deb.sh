#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

TAG_NAME="${1:?usage: package_release_deb.sh <tag_name> [platform_tag] [output_dir]}"
PLATFORM_TAG="${2:-linux-x86_64}"
OUTPUT_DIR="${3:-${REPO_ROOT}/dist/local-release}"

if [ "${PLATFORM_TAG}" != "linux-x86_64" ]; then
  echo "package_release_deb only supports linux-x86_64 at this time" >&2
  exit 1
fi

safe_tag="${TAG_NAME//\//-}"
release_dir="${OUTPUT_DIR}/${TAG_NAME}"
package_version="${TAG_NAME#v}"
if [ -z "${package_version}" ]; then
  echo "invalid tag name '${TAG_NAME}'" >&2
  exit 1
fi
safe_version="${package_version//[^a-zA-Z0-9.+~:-]/-}"
dpkg_version="${safe_version}"
if ! command -v dpkg-deb >/dev/null 2>&1; then
  echo "dpkg-deb is required for .deb packaging" >&2
  exit 1
fi

api_name="secureagnt-api-${PLATFORM_TAG}-${safe_tag}"
worker_name="secureagntd-${PLATFORM_TAG}-${safe_tag}"
ctl_name="agntctl-${PLATFORM_TAG}-${safe_tag}"

mkdir -p "${release_dir}"

if [ ! -x "${release_dir}/${api_name}" ]; then
  echo "[release-deb] prebuilt api binary not found at ${release_dir}/${api_name}, rebuilding..."
  cargo build --release -p api --bin secureagnt-api
  cp target/release/secureagnt-api "${release_dir}/${api_name}"
fi

if [ ! -x "${release_dir}/${worker_name}" ]; then
  echo "[release-deb] prebuilt worker binary not found at ${release_dir}/${worker_name}, rebuilding..."
  cargo build --release -p worker --bin secureagntd
  cp target/release/secureagntd "${release_dir}/${worker_name}"
fi

if [ ! -x "${release_dir}/${ctl_name}" ]; then
  echo "[release-deb] prebuilt cli binary not found at ${release_dir}/${ctl_name}, rebuilding..."
  cargo build --release -p agntctl
  cp target/release/agntctl "${release_dir}/${ctl_name}"
fi

staging_dir="${release_dir}/.deb-workspace"
package_root="${staging_dir}/secureagnt_${dpkg_version}_amd64"
deb_file="${release_dir}/secureagnt_${dpkg_version}_amd64.deb"
unit_dir="/lib/systemd/system"

rm -rf "${staging_dir}"
mkdir -p "${package_root}/DEBIAN" "${package_root}/usr/local/bin" "${package_root}/etc/secureagnt" "${package_root}/var/lib/secureagnt" "${package_root}/var/log/secureagnt" "${package_root}${unit_dir}"

cat > "${package_root}/DEBIAN/control" <<EOF
Package: secureagnt
Version: ${dpkg_version}
Section: net
Priority: optional
Architecture: amd64
Maintainer: SecureAgnt
Description: SecureAgnt Rust agent platform services
 SecureAgnt API and worker binaries plus supporting launch files.
Depends: libc6 (>= 2.31)
EOF

cat > "${package_root}/DEBIAN/preinst" <<'EOF'
#!/bin/sh
set -e

if ! id -u secureagnt >/dev/null 2>&1; then
  adduser --system \
    --home /var/lib/secureagnt \
    --shell /usr/sbin/nologin \
    secureagnt
fi
exit 0
EOF

cat > "${package_root}/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e

mkdir -p /var/lib/secureagnt /var/log/secureagnt
chown -R secureagnt:secureagnt /var/lib/secureagnt /var/log/secureagnt /etc/secureagnt

if command -v systemctl >/dev/null 2>&1; then
  systemctl daemon-reload
  if [ "$1" = "configure" ]; then
    systemctl enable secureagnt.service secureagnt-api.service || true
  fi
fi

exit 0
EOF

cat > "${package_root}/DEBIAN/prerm" <<'EOF'
#!/bin/sh
set -e

if [ "$1" = "remove" ] || [ "$1" = "deconfigure" ]; then
  if command -v systemctl >/dev/null 2>&1; then
    systemctl stop secureagnt.service secureagnt-api.service || true
    systemctl disable secureagnt.service secureagnt-api.service || true
  fi
fi

if command -v systemctl >/dev/null 2>&1; then
  systemctl daemon-reload || true
fi
exit 0
EOF

cat > "${package_root}/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e

if [ "$1" = "purge" ]; then
  userdel --remove secureagnt >/dev/null 2>&1 || true
fi

if command -v systemctl >/dev/null 2>&1; then
  systemctl daemon-reload || true
fi
exit 0
EOF

chmod 0755 \
  "${package_root}/DEBIAN/preinst" \
  "${package_root}/DEBIAN/postinst" \
  "${package_root}/DEBIAN/prerm" \
  "${package_root}/DEBIAN/postrm"

cp "${release_dir}/${api_name}" "${package_root}/usr/local/bin/secureagnt-api"
cp "${release_dir}/${worker_name}" "${package_root}/usr/local/bin/secureagntd"
cp "${release_dir}/${ctl_name}" "${package_root}/usr/local/bin/agntctl"

chmod +x \
  "${package_root}/usr/local/bin/secureagnt-api" \
  "${package_root}/usr/local/bin/secureagntd" \
  "${package_root}/usr/local/bin/agntctl"

cat > "${package_root}/etc/secureagnt/secureagnt.env" <<EOF
# secureagnt default environment (fill in as needed before starting services)
API_ADDR=0.0.0.0:8080
DATABASE_URL=
WORKER_CONCURRENCY=2
WORKER_ENABLE_LOCAL_EXEC=true
EOF

cp "${REPO_ROOT}/infra/systemd/secureagnt.service" "${package_root}${unit_dir}/secureagnt.service"
cp "${REPO_ROOT}/infra/systemd/secureagnt-api.service" "${package_root}${unit_dir}/secureagnt-api.service"

mkdir -p "${package_root}/usr/share/doc/secureagnt"

cat > "${package_root}/usr/share/doc/secureagnt/changelog" <<EOF
secureagnt (${dpkg_version}) stable; urgency=medium

  * Release package build for ${TAG_NAME}

 -- SecureAgnt <ops@secureagnt.ai>  $(date -R)
EOF

dpkg-deb --build --root-owner-group "${package_root}" "${deb_file}"

if command -v sha256sum >/dev/null 2>&1; then
  HASH_CMD=(sha256sum)
elif command -v shasum >/dev/null 2>&1; then
  HASH_CMD=(shasum -a 256)
else
  echo "[release-deb] missing hash tool (sha256sum/shasum required)" >&2
  exit 1
fi

manifest_file="${release_dir}/release-manifest.sha256"
if [ -f "${manifest_file}" ]; then
  "${HASH_CMD[@]}" "${deb_file}" >> "${manifest_file}"
else
  : > "${manifest_file}"
  "${HASH_CMD[@]}" "${deb_file}" >> "${manifest_file}"
fi

echo "[release-deb] built: ${deb_file}"
echo "[release-deb] manifest: ${manifest_file}"
ls -l "${release_dir}"/secureagnt_*.deb
echo "DEB_FILE=${deb_file}"
