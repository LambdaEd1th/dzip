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
 * Startup and shutdown code that allows derbh archives to be mount
 * automatically.
 *
 * This file is included as source to prevent it being excluded from the
 * link.
 */

extern "C" void _DerbhAutoInit();
extern "C" void _DerbhAutoTerminate();

struct AutoDerbh
{
    AutoDerbh()
    {
        _DerbhAutoInit();
    }

    ~AutoDerbh()
    {
        _DerbhAutoTerminate();
    }
};

static AutoDerbh autoderbh;
