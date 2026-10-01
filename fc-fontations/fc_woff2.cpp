/*
 * fontconfig/fc-fontations/fc_woff2.cpp
 *
 * Copyright 2026 Google LLC.
 *
 * Permission to use, copy, modify, distribute, and sell this software and its
 * documentation for any purpose is hereby granted without fee, provided that
 * the above copyright notice appear in all copies and that both that
 * copyright notice and this permission notice appear in supporting
 * documentation, and that the name of the author(s) not be used in
 * advertising or publicity pertaining to distribution of the software without
 * specific, written prior permission.  The authors make no
 * representations about the suitability of this software for any purpose.  It
 * is provided "as is" without express or implied warranty.
 *
 * THE AUTHOR(S) DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS SOFTWARE,
 * INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS, IN NO
 * EVENT SHALL THE AUTHOR(S) BE LIABLE FOR ANY SPECIAL, INDIRECT OR
 * CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE,
 * DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
 * TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
 * PERFORMANCE OF THIS SOFTWARE.
 */

#include "fc_woff2.h"

#ifdef HAVE_LIBWOFF2DEC
#include <woff2/decode.h>
#include <woff2/output.h>

extern "C" {

size_t
fc_woff2_decoded_size (const uint8_t *data,
                       size_t         length)
{
    if (!data || !length)
        return 0;
    return woff2::ComputeWOFF2FinalSize (data, length);
}

bool
fc_woff2_decode (const uint8_t *data,
                 size_t         length,
                 uint8_t       *out_data,
                 size_t         out_capacity,
                 size_t        *out_length)
{
    if (!data || !length || !out_data || !out_capacity || !out_length)
        return false;

    woff2::WOFF2MemoryOut out_stream (out_data, out_capacity);
    if (!woff2::ConvertWOFF2ToTTF (data, length, &out_stream))
        return false;

    *out_length = out_stream.Size ();
    return true;
}

}
#endif
