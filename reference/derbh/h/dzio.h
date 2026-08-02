#pragma once

/*
 * (C) 2001-2012 Marmalade. All Rights Reserved.
 *
 * This document is protected by copyright, and contains information
 * proprietary to Marmalade.
 *
 * This file consists of source code released by Marmalade under
 * the terms of the accompanying End User License Agreement (EULA).
 * Please do not use this program/source code before you have read the
 * EULA and have agreed to be bound by its terms.
 */

//-----------------------------------------------------------------------------
/*!
    file dzio.h

*/
//-----------------------------------------------------------------------------

#ifndef DZIO_H
#define DZIO_H

// Include the new header
#include "derbh.h"

#ifdef _UNICODE
#define _tInitialiseArchive _wInitialiseArchive
#define _tdzopen _wdzopen
#else
#define _tInitialiseArchive InitialiseArchive
#define _tdzopen dzopen
#endif

// LEGACY API
#define InitialiseArchive dzArchiveAttach
#define InitialiseArchiveFromMemory dzArchiveAttachFromMem
#define UnInitialiseArchive dzArchiveDetach
#define UnInitialiseSpecificArchive dzArchiveDetachNamed
#define SetDerbhAllocFree dzSetAllocFreeCallbacks

#ifdef __cplusplus
typedef dzAllocCallback DerbhAllocFunc;
typedef dzFreeCallback DerbhFreeFunc;
#endif

#endif // !DZIO_H
