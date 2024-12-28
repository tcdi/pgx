//LICENSE Portions Copyright 2019-2021 ZomboDB, LLC.
//LICENSE
//LICENSE Portions Copyright 2021-2023 Technology Concepts & Design, Inc.
//LICENSE
//LICENSE Portions Copyright 2023-2023 PgCentral Foundation, Inc. <contact@pgcentral.org>
//LICENSE
//LICENSE All rights reserved.
//LICENSE
//LICENSE Use of this source code is governed by the MIT license that can be found in the LICENSE file.

//! This module contains implementations to make it easier to work with Postgres' geometric types
//! in Rust.
//!
//! Simple types that support `Copy` are re-exported from the `pg_sys` crate and have `IntoDatum`
//! and `FromDatum` trait implementations. Variable Length types have zero-copy and owned structs
//! that have `IntoDatum` and `FromDatum` trait implementations.
//!
//! See the [Postgres docs](https://www.postgresql.org/docs/current/datatype-geometric.html) for
//! more information on the geometric types.
use std::{mem, ptr};

use pgrx_pg_sys::pfree;
use pgrx_sql_entity_graph::metadata::{
    ArgumentError, Returns, ReturnsError, SqlMapping, SqlTranslatable,
};

use crate::{pg_sys, set_varsize_4b, FromDatum, IntoDatum, PgMemoryContexts};

// Copy types

pub type Box = pg_sys::BOX;

impl FromDatum for Box {
    unsafe fn from_polymorphic_datum(
        datum: pg_sys::Datum,
        is_null: bool,
        _: pg_sys::Oid,
    ) -> Option<Self>
    where
        Self: Sized,
    {
        if is_null {
            None
        } else {
            let the_box = datum.cast_mut_ptr::<Self>();
            Some(the_box.read())
        }
    }
}

impl IntoDatum for Box {
    fn into_datum(mut self) -> Option<pg_sys::Datum> {
        unsafe {
            let ptr = PgMemoryContexts::CurrentMemoryContext
                .copy_ptr_into(&mut self, std::mem::size_of::<Self>());
            Some(ptr.into())
        }
    }

    fn type_oid() -> pg_sys::Oid {
        pg_sys::BOXOID
    }
}

pub type Line = pg_sys::LINE;

impl FromDatum for Line {
    unsafe fn from_polymorphic_datum(
        datum: pg_sys::Datum,
        is_null: bool,
        _: pg_sys::Oid,
    ) -> Option<Self>
    where
        Self: Sized,
    {
        if is_null {
            None
        } else {
            let line = datum.cast_mut_ptr::<Self>();
            Some(line.read())
        }
    }
}

impl IntoDatum for Line {
    fn into_datum(mut self) -> Option<pg_sys::Datum> {
        unsafe {
            let ptr = PgMemoryContexts::CurrentMemoryContext
                .copy_ptr_into(&mut self, std::mem::size_of::<Self>());
            Some(ptr.into())
        }
    }

    fn type_oid() -> pg_sys::Oid {
        pg_sys::LINEOID
    }
}

pub type LineSegment = pg_sys::LSEG;

impl FromDatum for LineSegment {
    unsafe fn from_polymorphic_datum(
        datum: pg_sys::Datum,
        is_null: bool,
        _: pg_sys::Oid,
    ) -> Option<Self>
    where
        Self: Sized,
    {
        if is_null {
            None
        } else {
            let lseg = datum.cast_mut_ptr::<Self>();
            Some(lseg.read())
        }
    }
}

impl IntoDatum for LineSegment {
    fn into_datum(mut self) -> Option<pg_sys::Datum> {
        unsafe {
            let ptr = PgMemoryContexts::CurrentMemoryContext
                .copy_ptr_into(&mut self, std::mem::size_of::<Self>());
            Some(ptr.into())
        }
    }

    fn type_oid() -> pg_sys::Oid {
        pg_sys::LSEGOID
    }
}

pub type Point = pg_sys::Point;

impl FromDatum for Point {
    unsafe fn from_polymorphic_datum(
        datum: pg_sys::Datum,
        is_null: bool,
        _: pg_sys::Oid,
    ) -> Option<Self>
    where
        Self: Sized,
    {
        if is_null {
            None
        } else {
            let point: *mut Self = datum.cast_mut_ptr();
            Some(point.read())
        }
    }
}

impl IntoDatum for Point {
    fn into_datum(mut self) -> Option<pg_sys::Datum> {
        unsafe {
            let copy = PgMemoryContexts::CurrentMemoryContext
                .copy_ptr_into(&mut self, std::mem::size_of::<Self>());
            Some(copy.into())
        }
    }

    fn type_oid() -> pg_sys::Oid {
        pg_sys::POINTOID
    }
}

pub type Circle = pg_sys::CIRCLE;

impl FromDatum for Circle {
    unsafe fn from_polymorphic_datum(
        datum: pg_sys::Datum,
        is_null: bool,
        _: pg_sys::Oid,
    ) -> Option<Self>
    where
        Self: Sized,
    {
        if is_null {
            None
        } else {
            let circle = datum.cast_mut_ptr::<Self>();
            Some(circle.read())
        }
    }
}

impl IntoDatum for Circle {
    fn into_datum(mut self) -> Option<pg_sys::Datum> {
        unsafe {
            let ptr = PgMemoryContexts::CurrentMemoryContext
                .copy_ptr_into(&mut self, std::mem::size_of::<Self>());
            Some(ptr.into())
        }
    }

    fn type_oid() -> pg_sys::Oid {
        pg_sys::CIRCLEOID
    }
}

// Variable Length Types

/// An owned Postgres `path` type.
#[derive(Debug, Clone, Default)]
pub struct Path {
    points: Vec<Point>,
    closed: bool,
}

impl Path {
    pub fn new(points: Vec<pg_sys::Point>, closed: bool) -> Self {
        Self { points, closed }
    }

    pub fn points(&self) -> &[Point] {
        &self.points
    }

    pub fn closed(&self) -> bool {
        self.closed
    }
}

impl IntoDatum for Path {
    fn into_datum(self) -> Option<pg_sys::Datum> {
        let num_points = self.points.len();
        let reserve_size = num_points * mem::size_of::<Point>();
        // 16 bytes for the header (4 for varsize, 4 for npts, 4 for closed, 4 for padding)
        let total_size = reserve_size + 16;

        unsafe {
            let path = PgMemoryContexts::CurrentMemoryContext
                .palloc(total_size as usize)
                .cast::<pg_sys::PATH>();
            set_varsize_4b(path.cast(), total_size as i32);
            (*path).npts = num_points as i32;
            (*path).closed = self.closed as i32;
            let points = (*path).p.as_mut_slice(num_points);
            for (i, point) in self.points.iter().enumerate() {
                points[i] = *point;
            }
            Some(pg_sys::Datum::from(path))
        }
    }

    fn type_oid() -> pg_sys::Oid {
        pg_sys::PATHOID
    }
}

impl FromDatum for Path {
    unsafe fn from_polymorphic_datum(
        datum: pg_sys::Datum,
        is_null: bool,
        _: pg_sys::Oid,
    ) -> Option<Self>
    where
        Self: Sized,
    {
        if is_null {
            None
        } else {
            let data = pg_sys::pg_detoast_datum(datum.cast_mut_ptr()).cast::<pg_sys::PATH>();
            let closed = (*data).closed != 0;
            let points = ptr::addr_of!((*data).p);
            let points = (*points).as_slice((*data).npts as usize).to_vec();
            if data != datum.cast_mut_ptr() {
                pfree(data.cast());
            }
            Some(Path { points, closed })
        }
    }
}

unsafe impl SqlTranslatable for Path {
    fn argument_sql() -> Result<SqlMapping, ArgumentError> {
        Ok(SqlMapping::literal("path"))
    }

    fn return_sql() -> Result<Returns, ReturnsError> {
        Ok(Returns::One(SqlMapping::literal("path")))
    }
}

/// An owned Postgres `polygon` type.
#[derive(Debug, Clone, Default)]
pub struct Polygon {
    points: Vec<Point>,
    boundbox: Box,
}

unsafe impl SqlTranslatable for Polygon {
    fn argument_sql() -> Result<SqlMapping, ArgumentError> {
        Ok(SqlMapping::literal("polygon"))
    }

    fn return_sql() -> Result<Returns, ReturnsError> {
        Ok(Returns::One(SqlMapping::literal("polygon")))
    }
}

impl Polygon {
    pub fn new(points: Vec<pg_sys::Point>) -> Self {
        // We determine our boundbox by finding the min/max x/y values of all the points
        // Fold over the points, updating the min/max as we go
        let Some(boundbox) = points.iter().fold(None, |acc, point| {
            let boundbox = acc.unwrap_or_else(|| Box { high: *point, low: *point });
            Some(Box {
                high: Point { x: boundbox.high.x.max(point.x), y: boundbox.high.y.max(point.y) },
                low: Point { x: boundbox.low.x.min(point.x), y: boundbox.low.y.min(point.y) },
            })
        }) else {
            return Self::default();
        };
        Self { points, boundbox }
    }

    pub fn points(&self) -> &[Point] {
        &self.points
    }

    pub fn boundbox(&self) -> Box {
        self.boundbox
    }
}

impl IntoDatum for Polygon {
    fn into_datum(self) -> Option<pg_sys::Datum> {
        let num_points = self.points.len();
        let reserve_size = num_points * mem::size_of::<Point>();
        // 40 bytes for the header (4 for varsize, 4 for npts, 32 for boundbox)
        let total_size = reserve_size + 40;

        unsafe {
            let polygon = PgMemoryContexts::CurrentMemoryContext
                .palloc(total_size as usize)
                .cast::<pg_sys::POLYGON>();
            set_varsize_4b(polygon.cast(), total_size as i32);
            (*polygon).npts = num_points as i32;
            let points = (*polygon).p.as_mut_slice(num_points);
            for (i, point) in self.points.iter().enumerate() {
                points[i] = *point;
            }
            (*polygon).boundbox = self.boundbox;
            Some(pg_sys::Datum::from(polygon))
        }
    }

    fn type_oid() -> pg_sys::Oid {
        pg_sys::POLYGONOID
    }
}

impl FromDatum for Polygon {
    unsafe fn from_polymorphic_datum(
        datum: pg_sys::Datum,
        is_null: bool,
        _: pg_sys::Oid,
    ) -> Option<Self>
    where
        Self: Sized,
    {
        if is_null {
            None
        } else {
            let data = pg_sys::pg_detoast_datum(datum.cast_mut_ptr()).cast::<pg_sys::POLYGON>();
            let points = ptr::addr_of!((*data).p);
            let points = (*points).as_slice((*data).npts as usize).to_vec();
            let boundbox = (*data).boundbox;
            if data != datum.cast_mut_ptr() {
                pfree(data.cast());
            }
            Some(Polygon { points, boundbox })
        }
    }
}
