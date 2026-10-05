/* Copyright (C) 2026 fontconfig Authors */
/* SPDX-License-Identifier: HPND */

#include "fcint.h"

#include <stdint.h>

static unsigned int
fc_generic_family_hash (register const char *str, register FC_GPERF_SIZE_T len);

static const struct FcGenericFamilyEntry *
fc_generic_family_lookup (register const char *str, register FC_GPERF_SIZE_T len);

#define GPERF_DOWNCASE    1
#define GPERF_CASE_STRCMP 1
static int
gperf_case_strcmp (register const char *s1, register const char *s2)
{
    return FcStrCmpIgnoreBlanksAndCase ((const FcChar8 *)s1, (const FcChar8 *)s2);
}

#include "fcgenericfamily.h"

uint32_t
FcGenericAliasGetClassification (const char *family)
{
    const struct FcGenericFamilyEntry *entry;
    size_t                             len;
    uint32_t                           result = FC_FAMILY_UNKNOWN;

    if (!family)
	return FC_FAMILY_UNKNOWN;

    len = strlen (family);
    entry = fc_generic_family_lookup (family, len);
    if (entry)
	result = entry->classification;

    return result;
}

/*
 * Derive the genericfamily integer values for a list of family-name
 * values, using ONLY the curated classification data.
 *
 * Used by the matcher to reconstruct genericfamily for fonts loaded from
 * caches that predate the genericfamily object (issue #562), so those
 * fonts score like freshly scanned ones for families we can classify
 * reliably.  Unlike the scanner (fcfreetype.c) it deliberately does NOT
 * guess a generic from substrings of the family name ("mono", "sans",
 * ...): that heuristic is prone to false positives, so a family we cannot
 * classify is reported as FC_FAMILY_UNKNOWN rather than guessed.
 *
 * The result is returned by value; its fixed-size array makes it
 * impossible to overflow (the bit loop and the array share the same
 * bound), so there is no caller-provided buffer to size wrong.
 */
FcGenericFamilyValues
FcGenericFamilyGetValues (FcValueListPtr families)
{
    FcGenericFamilyValues r;
    FcValueListPtr        l;

    r.n = 0;
    /* Stop at the first family we can classify, like the scanner does. */
    for (l = families; l && r.n == 0; l = FcValueListNext (l)) {
	FcValue  v = FcValueCanonicalize (&l->value);
	uint32_t field;
	int      b;

	if (v.type != FcTypeString)
	    continue;
	field = FcGenericAliasGetClassification ((const char *)v.u.s);
	for (b = 0; b < FC_GENERIC_FAMILY_MAX_VALUES; b++)
	    if ((field & (1 << b)) != 0)
		r.values[r.n++] = b + 1;
    }
    /* No family could be classified. */
    if (r.n == 0)
	r.values[r.n++] = FC_FAMILY_UNKNOWN;

    return r;
}
