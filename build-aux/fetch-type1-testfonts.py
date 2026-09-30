#!/usr/bin/env python3

import argparse
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

# Packages and the selected complex Type 1 font files they contain:
TARGET_FONTS_BY_RPM = {
    "Packages/t/texlive-algolrevived-svn56864-76.fc42.noarch.rpm": [
        "AlgolRevived-Medium.pfb",
        "AlgolRevived-MediumSlanted.pfb",
    ],
    "Packages/t/texlive-dejavu-svn31771.2.34-76.fc42.noarch.rpm": [
        "DejaVuSans.pfb",
        "DejaVuSansCondensed.pfb",
    ],
    "Packages/t/texlive-txfontsb-svn54512-76.fc42.noarch.rpm": [
        "FreeSerifb.pfb",
    ],
    "Packages/t/texlive-stix2-type1-svn57448-76.fc42.noarch.rpm": [
        "STIX2Math.pfb",
    ],
    "Packages/t/texlive-libertinust1math-svn61751-76.fc42.noarch.rpm": [
        "LibertinusT1Math.pfb",
    ],
    "Packages/t/texlive-stix-svn54512-76.fc42.noarch.rpm": [
        "STIXGeneral-Regular.pfb",
    ],
    "Packages/t/texlive-step-svn57307-76.fc42.noarch.rpm": [
        "STEP-Regular.pfb",
    ],
    "Packages/x/xorg-x11-fonts-Type1-7.5-40.fc42.noarch.rpm": [
        "UTBI____.pfa",
    ],
}

EXPECTED_RPM_HASHES = {
    "texlive-algolrevived-svn56864-76.fc42.noarch.rpm": "a55985cabcfab62ddb2dcf5af0b97f4f8ec77293a719434960652ea2f73495ce",
    "texlive-dejavu-svn31771.2.34-76.fc42.noarch.rpm": "df3abb852fdb68b2fb41def8dda4c8f17193ebde97b1f8788c5a4313cd27cf27",
    "texlive-txfontsb-svn54512-76.fc42.noarch.rpm": "dc6d04b35d25b0e32e2297b6984344e9b97eb525e24f5a37f6a222b5fa5a8bd9",
    "texlive-stix2-type1-svn57448-76.fc42.noarch.rpm": "5a1a4c2c32b892b774004a776832a1e39fc0671b6fbdf7998c5d7e6eefe2d52a",
    "texlive-libertinust1math-svn61751-76.fc42.noarch.rpm": "d49414ad94221e130781262d17709e2becfc353f39e114e3ec9e300eedec4ab7",
    "texlive-stix-svn54512-76.fc42.noarch.rpm": "651444a067f180279297d2ab156dbf76f51ec6aad13e0af1ef6d36a77bc39e39",
    "texlive-step-svn57307-76.fc42.noarch.rpm": "e1eeab9bab94bae5e6647b76d96792978e66a9be97b13af3aede3a2c5ff0aca9",
    "xorg-x11-fonts-Type1-7.5-40.fc42.noarch.rpm": "e4831023d5a7e50ec6ba04f03a9142b1d499f1df6b9b0c5e18c7cdcf87b00e01",
}

# 10 most complex Type 1 fonts determined by grading parsing results across Fedora package archives
EXPECTED_HASHES = """
AlgolRevived-Medium.pfb         1e3caa39d40876d46c28dd3cb0b06b5d95a32edd68db3f5e9970a20979691c77
AlgolRevived-MediumSlanted.pfb  274dd9c306ffea55a8189cd87c75011f924972c9757b00892620e99ea8fd3749
DejaVuSans.pfb                  c6f1fc6c7e378147001d38739ce7424ec38ba7c751f105f4d1054a18ffc4826a
DejaVuSansCondensed.pfb         adb488719b447c1c40605500aca54eb5b321500180382b795f9e863f45c0b506
FreeSerifb.pfb                  cace0901a9829f09840150a4a58680020b51fb0ef2cb09f7b824dcfdb0500334
LibertinusT1Math.pfb            e45ca70d60a0d66e5f795a2796ebfd12d62b082b51cb1c51a8a5ae68a74fb9ea
STEP-Regular.pfb                bca59916bfe6029110b532dd8732a1df6e31159f4930a719c2efa5045aa84505
STIX2Math.pfb                   18fa7dd459589a5090bf7ec1e356c8a211844481204af29c9a2bc69848d724f6
STIXGeneral-Regular.pfb         8e6d3522702eb37f3b746deb6747a1e020937ddb279a4acb423a3b09b33e3150
UTBI____.pfa                    eddb876641135c995145c1401497009f0ba0d6eda6d02dc9dde57ff322f65cf9
"""

CONTAINER_DOWNLOAD_DIR = "/type1testfonts"
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
        description="Download and extract complex Type 1 fonts from Fedora package archives."
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
                    f"Symlinked Type 1 test fonts directory from {CONTAINER_DOWNLOAD_DIR} to {target_dir}."
                )
                return 0
            except FileExistsError:
                logger.debug(
                    f"Target directory {target_dir} already exists. Skipping symlink."
                )
            except OSError as e:
                logger.warning(f"Failed to create symlink: {e}")
        else:
            logger.debug("Pre-downloaded Type 1 test fonts not found in container.")

    target_stamp = os.path.join(target_dir, STAMP_FILE)
    if stamp_hashes_match(target_stamp):
        logger.info("Type 1 test fonts already downloaded and extracted.")
        return 0

    if not has_rpm_unpack_support():
        logger.warning(
            "Neither command line tool (rpm2cpio/cpio) nor rpmfile library found to unpack RPMs. "
            "Skipping Type 1 font download."
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
            f"Successfully extracted and verified {len(all_font_hashes)} Type 1 test fonts."
        )


if __name__ == "__main__":
    main()
