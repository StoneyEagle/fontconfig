/*
 * fontconfig/fc-fontations/fc_woff2.h
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

#ifndef _FC_WOFF2_H_
#define _FC_WOFF2_H_

#ifdef HAVE_CONFIG_H
#undef _GNU_SOURCE
#include <config.h>
#endif

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef HAVE_LIBWOFF2DEC

size_t
fc_woff2_decoded_size (const uint8_t *data,
                       size_t         length);

bool
fc_woff2_decode (const uint8_t *data,
                 size_t         length,
                 uint8_t       *out_data,
                 size_t         out_capacity,
                 size_t        *out_length);

#endif /* HAVE_LIBWOFF2DEC */

#ifdef __cplusplus
}
#endif

#endif /* _FC_WOFF2_H_ */
