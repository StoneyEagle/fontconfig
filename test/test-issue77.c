/*
 * fontconfig/test/test-issue77.c
 *
 * Copyright © 2026 John Cardullo
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

#include <fontconfig/fontconfig.h>
#include <stdio.h>
#include <stdlib.h>
#include <assert.h>

/* Global pointers to keep heap-allocated objects reachable for leak checkers */
FcPattern *global_pat = NULL;
FcCharSet *global_cs = NULL;

static int
test_dynamic_charset (void)
{
    FcCharSet *cs1, *cs2, *union_cs, *sub_cs;
    FcChar32 cp;
    int i;

    cs1 = FcCharSetCreate ();
    if (!cs1)
	return 1;

    /* Add characters across many distinct Unicode pages to trigger
     * multiple reallocations of both leaves and numbers arrays. */
    for (i = 0; i < 64; i++)
    {
	/* Page i: character (i << 8) | 0x20 */
	cp = ((FcChar32)i << 8) | 0x20;
	if (!FcCharSetAddChar (cs1, cp))
	{
	    FcCharSetDestroy (cs1);
	    return 1;
	}
    }

    /* Verify all added characters are present */
    for (i = 0; i < 64; i++)
    {
	cp = ((FcChar32)i << 8) | 0x20;
	if (!FcCharSetHasChar (cs1, cp))
	{
	    FcCharSetDestroy (cs1);
	    return 1;
	}
	/* Verify characters not added are absent */
	if (FcCharSetHasChar (cs1, cp + 1))
	{
	    FcCharSetDestroy (cs1);
	    return 1;
	}
    }

    /* Test duplicate / set operations */
    cs2 = FcCharSetCopy (cs1);
    if (!FcCharSetEqual (cs1, cs2))
    {
	FcCharSetDestroy (cs1);
	FcCharSetDestroy (cs2);
	return 1;
    }
    FcCharSetDestroy (cs2);

    cs2 = FcCharSetCreate ();
    FcCharSetAddChar (cs2, 0x12345);
    union_cs = FcCharSetUnion (cs1, cs2);
    if (!union_cs || !FcCharSetHasChar (union_cs, 0x12345) || !FcCharSetHasChar (union_cs, 0x20))
    {
	FcCharSetDestroy (cs1);
	FcCharSetDestroy (cs2);
	if (union_cs)
	    FcCharSetDestroy (union_cs);
	return 1;
    }

    sub_cs = FcCharSetSubtract (union_cs, cs2);
    if (!sub_cs || !FcCharSetEqual (sub_cs, cs1))
    {
	FcCharSetDestroy (cs1);
	FcCharSetDestroy (cs2);
	FcCharSetDestroy (union_cs);
	if (sub_cs)
	    FcCharSetDestroy (sub_cs);
	return 1;
    }

    FcCharSetDestroy (cs2);
    FcCharSetDestroy (union_cs);
    FcCharSetDestroy (sub_cs);

    /* Keep cs1 in global pointer */
    global_cs = cs1;
    return 0;
}

static int
test_dynamic_pattern (void)
{
    FcPattern *p, *dup_p;
    FcChar8 *fam;
    int weight;
    double size;

    p = FcPatternCreate ();
    if (!p)
	return 1;

    if (!FcPatternAddString (p, FC_FAMILY, (const FcChar8 *)"TestSans"))
    {
	FcPatternDestroy (p);
	return 1;
    }
    if (!FcPatternAddInteger (p, FC_WEIGHT, FC_WEIGHT_BOLD))
    {
	FcPatternDestroy (p);
	return 1;
    }
    if (!FcPatternAddDouble (p, FC_SIZE, 14.5))
    {
	FcPatternDestroy (p);
	return 1;
    }
    if (!FcPatternAddCharSet (p, FC_CHARSET, global_cs))
    {
	FcPatternDestroy (p);
	return 1;
    }

    if (FcPatternGetString (p, FC_FAMILY, 0, &fam) != FcResultMatch ||
        FcStrCmp (fam, (const FcChar8 *)"TestSans") != 0)
    {
	FcPatternDestroy (p);
	return 1;
    }
    if (FcPatternGetInteger (p, FC_WEIGHT, 0, &weight) != FcResultMatch ||
        weight != FC_WEIGHT_BOLD)
    {
	FcPatternDestroy (p);
	return 1;
    }
    if (FcPatternGetDouble (p, FC_SIZE, 0, &size) != FcResultMatch ||
        size != 14.5)
    {
	FcPatternDestroy (p);
	return 1;
    }

    dup_p = FcPatternDuplicate (p);
    if (!dup_p || !FcPatternEqual (p, dup_p))
    {
	FcPatternDestroy (p);
	if (dup_p)
	    FcPatternDestroy (dup_p);
	return 1;
    }
    FcPatternDestroy (dup_p);

    /* Test printing dynamic pattern and its charset (FcPatternPrint / FcCharSetPrintFile) */
    FcPatternPrint (p);

    global_pat = p;
    return 0;
}

int
main (void)
{
    if (test_dynamic_charset () != 0)
    {
	fprintf (stderr, "Dynamic charset test failed\n");
	return 1;
    }
    if (test_dynamic_pattern () != 0)
    {
	fprintf (stderr, "Dynamic pattern test failed\n");
	return 1;
    }

    /*
     * Intentional leak check: do NOT free global_cs, global_pat, or call
     * FcFini(). The purpose of this test is to verify that LSan/ASan can
     * follow direct pointers from reachable root objects to their internal
     * arrays at process exit. Freeing them here would empty the heap and
     * prevent LSan from verifying reachability (see issue #77).
     */
    return 0;
}
