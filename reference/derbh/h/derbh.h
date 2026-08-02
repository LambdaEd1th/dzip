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
/*
 * Master Header File to include to access all Derbh APIs.
 */

#ifndef DERBH_H
#define DERBH_H

// Includes
#include "s3eTypes.h"

/**
 * @defgroup derbhgroup Derbh API Reference
 *
 * Derbh is an interface for creating and reading compressed Derbh archives (DZ files).
 * These archives are similar to ZIP files, but typically achieve a higher compression ratio
 * than ZIP when applied to collections of game assets.
 */

/**
 * @addtogroup derbhgroup
 * @{
 */

/**
 * @defgroup derbhDZ Derbh Low-level API
 *
 * For more information on the Derbh Low-level API, see the
 * /ref derbhlowleveloverview "Derbh Low-level API" section of the
 * <em>Derbh API Documentation</em>.
 *
 * @{
 */

// not all of this file is seen from .c files
#ifdef __cplusplus

// Forward declarations
class ArchiveManager;

//-----------------------------------------------------------------------------
// DZFILE
//-----------------------------------------------------------------------------
struct DZFILE
{
    int32 _curpos;              // current position in Derbh file
    int32 _curchunkstart;       // start of current chunk in Derbh file
    DZFILE *next;               // pointer to the next Derbh file
    uint16* _infopos;           // current info position
    uint8* _buf;                // current buffer
    int32 _bufoffset;           // current buffer offset
    int _buflen;                // current buffer length
    uint8 _err_eof;             // fast place to store the eof char
    ArchiveManager* pAManager;  // pointer to the Archive Manager
};

extern "C" {

/**
 * Typedef for callback function to allocate memory.
 * @see dzFreeCallback, dzSetAllocFreeCallbacks
 */
typedef void* (*dzAllocCallback)(uint32);

/**
 * Typedef for callback function to allocate memory.
 * @see dzFreeCallback, dzSetAllocFreeCallbacks
 */
typedef void (*dzFreeCallback)(void*);

/**
 * Set callbacks for Derbh to use your own memory alloc() and free() functions.
 * @param fnAlloc Function pointer to use for allocating memory.
 * @param fnFree Function pointer to use for deallocating memory .
 */
void dzSetAllocFreeCallbacks(dzAllocCallback fnAlloc, dzFreeCallback fnFree);

/**
 * Open a Derbh file.
 * @param filename Full pathname of file.
 * @param pMode stdio-type string defining the mode in which to open the file.
 * @result Pointer to a Derbh file object, or NULL if failed.
 * @see s3eFileOpen, s3eFileOpenFromMemory
 */
DZFILE* dzopen(const char* filename, const char* mode);

/**
 * Read some data from an open Derbh file.
 * @param pBuffer Pointer to the area in memory at which to start reading.
 * @param size Size in bytes of a single data element to be read.
 * @param count Number of data elements to read, in bytes.
 * @param dzFile Pointer to the Derbh file object to read from.
 * @result The full number of items actually read (same caveats as fread).
 * The total number of bytes read is (size * count).
 * @see s3eFileWrite, s3eFileOpen, s3eFileClose
 */
uint32 dzread(void* pBuffer, uint32 size, uint32 count, DZFILE* dzFile);

/**
 * Retrieve the last error on a Derbh file.
 * @param dzFile Pointer to the Derbh file object to read from.
 * @result 0 indicates no error.
 */
int32 dzerror(DZFILE* dzFile);

/**
 * Query end-of-dile on a Derbh file.
 * @param dzFile Pointer to the Derbh file object to read from.
 * @result true only if eof is reached.
 */
s3eBool dzeof(DZFILE* dzFile);

/**
 * Move the file pointer to a specified location.
 * @param dzFile Pointer to the Derbh file object.
 * @param offset Number of bytes from specified origin to seek to.
 * @param origin Origin type to use.
 * @result 0 if successful. Behaves exactly the same as fseek.
 */
int32 dzseek(DZFILE* dzFile, int32 offset, int32 origin);

/**
 * Get the current position of a file pointer.
 * @param dzFile Pointer to the Derbh file object.
 * @result The current file pointer offset, in bytes. Behaves exactly the same as ftell.
 * @see dztell
 */
int32 dztell(DZFILE* dzFile);

/**
 * Close an open Derbh file.
 * @param dzFile Pointer to the s3eFile object to close.
 * @result 0 if successful.
 * @see dzopen
 */
int32 dzclose(DZFILE* dzFile);

/**
 * Close all open Derbh files.
 * @result 0 if successful.
 * @see dzclose
 */
int32 dzcloseall();

#endif

/** @} */
/** @} */

/**
 * @addtogroup derbhgroup
 * @{
 */

/**
 * @defgroup derbhArchive Derbh High-level Archive API
 *
 * For more information on the Derbh High-level Archive API,
 * see the "Derbh High-level Archive API" section of the
 * <em>Derbh API Documentation</em>.
 *
 * @{
 */

//------------------------------------------------
// Archive Access System
//------------------------------------------------

/**
 * Attach an archive file to the archiving system.
 * This should be the main .dz file output by the compressor.
 * All other sub-archives specified in this main dz file will be loaded automatically.
 *
 * This stage mounts the Derbh custom s3eFile system. Subsequent calls
 * to s3eFile functions will operate on the full archive as a logical drive.
 *
 * You can attach multiple archives with this function. The files in additional archives
 * are added to the logical drive. Should a file already exist, the newer version replaces it.
 *
 * @param pArchiveName Archive name.
 * @result 0 if successful, otherwise non-zero error code
 * @see dzArchiveDetach, dzArchiveDetachNamed
 */
int32 dzArchiveAttach(const char* pArchiveName);

/**
 * Attach an archive file to the archiving system, from memory.
 * @param pMem Pointer to file in memory.
 * @param size File sizee in bytes.
 * @result 0 if successful, otherwise non-zero error code
 * @see dzArchiveDetach, dzArchiveDetachFromMem
 */
int32 dzArchiveAttachFromMem(void* pMem, uint32 size);

/**
 * Detach the last archive file which was attached.
 * The files of the last initialised archives are removed
 * from the logical file system.
 * If there are no more archives remaining then the custom
 * filesystem is un-mounted. s3e file functionality returns to
 * default.
 * @see dzArchiveAttach
 */
void dzArchiveDetach();

/**
 * Detach a named archive file.
 * The files in this archive are removed from the logical
 * file system.
 * If there are no more archives remaining then the custom
 * filesystem is un-mounted. s3e file functionality returns to
 * default.
 * @param pArchiveName Archive name.
 * @see dzArchiveAttach
 */
void dzArchiveDetachNamed(const char* pArchiveName);

/**
 * Detach a given memory block that has previously been attached as a derbh file.
 * @param pMem Pointer to memory block
 * @see dzArchiveAttachFromMem
 */
void dzArchiveDetachFromMem(void* pMem);


#ifdef __cplusplus
}
#endif

/** @} */
/** @} */

#endif // !DERBH_H
