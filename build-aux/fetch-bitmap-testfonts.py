#!/usr/bin/env python3

import argparse
import gzip
import hashlib
import logging
import multiprocessing
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.request

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger()

if not sys.stdout.isatty():
    logger.handlers = []

FEDORA_ARCHIVE_URL = (
    "https://archives.fedoraproject.org/pub/archive/fedora/linux/releases/42/Everything/x86_64/os/%s"
)

# Packages and the selected bitmap font files they contain:
TARGET_FONTS_BY_RPM = {
    "Packages/u/ucs-miscfixed-fonts-0.3-35.fc42.noarch.rpm": [
        "4x6.bdf",
        "6x13.bdf",
        "6x13B.bdf",
        "6x13O.bdf",
        "10x20.bdf",
    ],
    "Packages/x/xorg-x11-fonts-misc-7.5-40.fc42.noarch.rpm": [
        "6x13B.pcf",
        "10x20.pcf",
    ],
    "Packages/w/wine-fixedsys-fonts-10.4-1.fc42.noarch.rpm": [
        "vgafix.fon",
    ],
    "Packages/w/wine-courier-fonts-10.4-1.fc42.noarch.rpm": [
        "coure.fon",
    ],
}

EXPECTED_RPM_HASHES = {
    "ucs-miscfixed-fonts-0.3-35.fc42.noarch.rpm": "a3d1f0f3e33cf9eea94e038b0842ddce6b721f4b8c622ed955bab6d75f8cc6f5",
    "xorg-x11-fonts-misc-7.5-40.fc42.noarch.rpm": "cba995346ba450d999e236e50c1ec22bf0e12d43e3e14a377ac3104e0823ad6f",
    "wine-fixedsys-fonts-10.4-1.fc42.noarch.rpm": "60c359c3f9f4de7c3059b74264acd04190a3eba02314f77ba3a0b914888ea10c",
    "wine-courier-fonts-10.4-1.fc42.noarch.rpm": "6b823803df9a55fe7aa1800a6413383ea456f2252c7bcd0357b69b84ea8939d8",
}

EXPECTED_HASHES = """
10x20.bdf            3c302599929cf4c4ad6a867d11b12d53b2260e02bebb96c8c53f93797f027fa6
10x20.pcf            3bab1d661d2bf9d6150319c1e12be2ff569d639a1c9541971d0aee4902adc0f7
4x6.bdf              14677cf206c468d5cd13e57a7479da635058b3abaa50900249a2b4d88ea97c76
6x13.bdf             d00cf1cbaed055bb91d11c80e063d5d2d3b66552a7f2ead7161ee3780f9123f0
6x13B.bdf            ae9164892e84f871e9a0e89c3f026d150ec6d32af0bd9ddd0a103ed17af6cba2
6x13B.pcf            b4975115de7407e28cb92167823ffc2952ebfd6320d9a5e947fc839c822f90bc
6x13O.bdf            960bc2abf7f11bf5a25f2b8379dc67bc170c21498800886bad1546fcc3c7d85f
coure.fon            e55d2d1f38f85f6c182409a857e505eab71d053d24970c12c6cf0820760439b1
vgafix.fon           87b8914f116a09c37a47d67e23e41c52a59c6e7e7bc70c36844f1a92f262928d
"""

CONTAINER_DOWNLOAD_DIR = "/bitmaptestfonts"
STAMP_FILE = ".stamp"


def compute_sha256(filepath):
    """Computes the SHA-256 hash of a file."""
    hasher = hashlib.sha256()
    with open(filepath, "rb") as file:
        while True:
            chunk = file.read(4096)
            if not chunk:
                break
            hasher.update(chunk)
    return hasher.hexdigest()


def download_file(url, tmp_dir):
    filename = os.path.basename(url)
    filepath = os.path.join(tmp_dir, filename)
    logger.info(f"Downloading {url} to {filepath}")
    req = urllib.request.Request(url, headers={"User-Agent": "FontconfigTest/1.0"})
    with urllib.request.urlopen(req) as resp, open(filepath, "wb") as out:
        shutil.copyfileobj(resp, out)
    return filepath


def has_rpm_unpack_support():
    """Check if command-line unpacking tools or python libraries are available."""
    if shutil.which("rpm2cpio") and shutil.which("cpio"):
        return True
    try:
        import rpmfile  # noqa: F401
        return True
    except ImportError:
        return False


def extract_rpm_files(rpm_path, target_dir, wanted_fonts):
    """Extract specified files from an RPM archive using rpm2cpio + cpio, or rpmfile."""
    wanted_set = set(wanted_fonts)

    if shutil.which("rpm2cpio") and shutil.which("cpio"):
        with tempfile.TemporaryDirectory() as tmp_extract:
            p1 = subprocess.Popen(["rpm2cpio", rpm_path], stdout=subprocess.PIPE)
            p2 = subprocess.Popen(
                ["cpio", "-id", "--quiet"],
                stdin=p1.stdout,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                cwd=tmp_extract,
            )
            p1.stdout.close()
            _, err = p2.communicate()
            if p2.returncode != 0:
                raise RuntimeError(
                    f"cpio failed: {err.decode('utf-8', errors='ignore')}"
                )

            extracted_hashes = []
            for root, _, files in os.walk(tmp_extract):
                for f in files:
                    if f in wanted_set:
                        dest_path = os.path.join(target_dir, f)
                        shutil.copy2(os.path.join(root, f), dest_path)
                        extracted_hashes.append((f, compute_sha256(dest_path)))
                    elif f.endswith(".gz") and f[:-3] in wanted_set:
                        target_name = f[:-3]
                        dest_path = os.path.join(target_dir, target_name)
                        with open(os.path.join(root, f), "rb") as gz_in:
                            decompressed = gzip.decompress(gz_in.read())
                        with open(dest_path, "wb") as out_f:
                            out_f.write(decompressed)
                        extracted_hashes.append((target_name, compute_sha256(dest_path)))
            return extracted_hashes

    try:
        import rpmfile
        with rpmfile.open(rpm_path) as rpm:
            extracted_hashes = []
            for member in rpm.getmembers():
                basename = os.path.basename(member.name)
                if basename in wanted_set:
                    dest_path = os.path.join(target_dir, basename)
                    with open(dest_path, "wb") as f, rpm.extractfile(member) as src:
                        shutil.copyfileobj(src, f)
                    extracted_hashes.append((basename, compute_sha256(dest_path)))
                elif basename.endswith(".gz") and basename[:-3] in wanted_set:
                    target_name = basename[:-3]
                    dest_path = os.path.join(target_dir, target_name)
                    with rpm.extractfile(member) as src:
                        decompressed = gzip.decompress(src.read())
                    with open(dest_path, "wb") as f:
                        f.write(decompressed)
                    extracted_hashes.append((target_name, compute_sha256(dest_path)))
            return extracted_hashes
    except ImportError:
        pass

    raise RuntimeError(
        "Neither command line tools (rpm2cpio, cpio) nor rpmfile library found to unpack RPM."
    )


def download_and_extract_rpm(rel_url, wanted_fonts, tmp_dir, target_dir):
    full_url = FEDORA_ARCHIVE_URL % rel_url
    rpm_path = download_file(full_url, tmp_dir)
    rpm_filename = os.path.basename(rpm_path)

    # Verify RPM hash
    actual_hash = compute_sha256(rpm_path)
    expected_hash = EXPECTED_RPM_HASHES.get(rpm_filename)
    if expected_hash and actual_hash != expected_hash:
        raise ValueError(
            f"Hash mismatch for {rpm_filename}: expected {expected_hash}, got {actual_hash}"
        )

    logger.info(f"Extracting {wanted_fonts} from {rpm_filename}")
    font_hashes = extract_rpm_files(rpm_path, target_dir, wanted_fonts)
    extracted_names = {name for name, _ in font_hashes}
    for font_name in wanted_fonts:
        if font_name not in extracted_names:
            raise ValueError(f"Font {font_name} not found in {rpm_filename}")

    return font_hashes


def stamp_hashes_match(stamp_path):
    if not os.path.exists(stamp_path):
        return False

    with open(stamp_path, "r") as f:
        lines = f.readlines()

    expected = {}
    for line in EXPECTED_HASHES.strip().split("\n"):
        parts = line.split()
        if len(parts) == 2:
            expected[parts[0]] = parts[1]

    found = {}
    for line in lines:
        parts = line.split()
        if len(parts) == 2:
            found[parts[0]] = parts[1]

    if set(expected.keys()) != set(found.keys()):
        return False

    for k, v in expected.items():
        if found.get(k) != v:
            return False

    return True


def stamp_target_dir(target_dir, font_hashes):
    stamp_path = os.path.join(target_dir, STAMP_FILE)
    if os.path.exists(stamp_path):
        os.remove(stamp_path)

    max_len = max(len(name) for name, _ in font_hashes) if font_hashes else 0
    content = "\n".join(f"{name:<{max_len}} {h}" for name, h in sorted(font_hashes))
    with open(stamp_path, "w") as f:
        f.write(content + "\n")

    if not stamp_hashes_match(stamp_path):
        raise ValueError("Extracted fonts do not match expected stamp hashes!")


def main():
    parser = argparse.ArgumentParser(
        description="Download and extract bitmap test fonts (BDF, PCF, FON) from Fedora package archives."
    )
    parser.add_argument(
        "--target-dir", required=True, help="Target directory for extracted fonts."
    )
    parser.add_argument(
        "--try-symlink",
        action="store_true",
        help="Try to symlink test files previously downloaded to the container.",
    )
    args = parser.parse_args()

    target_dir = args.target_dir

    if args.try_symlink:
        container_stamp = os.path.join(CONTAINER_DOWNLOAD_DIR, STAMP_FILE)
        if os.path.exists(CONTAINER_DOWNLOAD_DIR) and stamp_hashes_match(
            container_stamp
        ):
            try:
                os.symlink(CONTAINER_DOWNLOAD_DIR, target_dir, target_is_directory=True)
                logger.info(
                    f"Symlinked bitmap test fonts directory from {CONTAINER_DOWNLOAD_DIR} to {target_dir}."
                )
                return 0
            except FileExistsError:
                logger.debug(
                    f"Target directory {target_dir} already exists. Skipping symlink."
                )
            except OSError as e:
                logger.warning(f"Failed to create symlink: {e}")
        else:
            logger.debug("Pre-downloaded bitmap test fonts not found in container.")

    target_stamp = os.path.join(target_dir, STAMP_FILE)
    if stamp_hashes_match(target_stamp):
        logger.info("Bitmap test fonts already downloaded and extracted.")
        return 0

    if not has_rpm_unpack_support():
        logger.warning(
            "Neither command line tool (rpm2cpio/cpio) nor rpmfile library found to unpack RPMs. "
            "Skipping bitmap font download."
        )
        return 0

    os.makedirs(target_dir, exist_ok=True)

    with tempfile.TemporaryDirectory() as tmp_dir:
        tasks = [
            (rel_url, wanted_fonts, tmp_dir, target_dir)
            for rel_url, wanted_fonts in TARGET_FONTS_BY_RPM.items()
        ]
        with multiprocessing.Pool(
            processes=min(len(tasks), multiprocessing.cpu_count())
        ) as pool:
            results = pool.starmap(download_and_extract_rpm, tasks)

        all_font_hashes = [item for sublist in results for item in sublist]
        stamp_target_dir(target_dir, all_font_hashes)
        logger.info(
            f"Successfully extracted and verified {len(all_font_hashes)} bitmap test fonts."
        )


if __name__ == "__main__":
    main()
