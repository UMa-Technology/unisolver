// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'types.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$CalibModelDto {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is CalibModelDto);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'CalibModelDto()';
}


}

/// @nodoc
class $CalibModelDtoCopyWith<$Res>  {
$CalibModelDtoCopyWith(CalibModelDto _, $Res Function(CalibModelDto) __);
}


/// Adds pattern-matching-related methods to [CalibModelDto].
extension CalibModelDtoPatterns on CalibModelDto {
/// A variant of `map` that fallback to returning `orElse`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( CalibModelDto_Radial value)?  radial,TResult Function( CalibModelDto_Polynomial value)?  polynomial,required TResult orElse(),}){
final _that = this;
switch (_that) {
case CalibModelDto_Radial() when radial != null:
return radial(_that);case CalibModelDto_Polynomial() when polynomial != null:
return polynomial(_that);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// Callbacks receives the raw object, upcasted.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case final Subclass2 value:
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( CalibModelDto_Radial value)  radial,required TResult Function( CalibModelDto_Polynomial value)  polynomial,}){
final _that = this;
switch (_that) {
case CalibModelDto_Radial():
return radial(_that);case CalibModelDto_Polynomial():
return polynomial(_that);}
}
/// A variant of `map` that fallback to returning `null`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( CalibModelDto_Radial value)?  radial,TResult? Function( CalibModelDto_Polynomial value)?  polynomial,}){
final _that = this;
switch (_that) {
case CalibModelDto_Radial() when radial != null:
return radial(_that);case CalibModelDto_Polynomial() when polynomial != null:
return polynomial(_that);case _:
  return null;

}
}
/// A variant of `when` that fallback to an `orElse` callback.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  radial,TResult Function( int order)?  polynomial,required TResult orElse(),}) {final _that = this;
switch (_that) {
case CalibModelDto_Radial() when radial != null:
return radial();case CalibModelDto_Polynomial() when polynomial != null:
return polynomial(_that.order);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// As opposed to `map`, this offers destructuring.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case Subclass2(:final field2):
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  radial,required TResult Function( int order)  polynomial,}) {final _that = this;
switch (_that) {
case CalibModelDto_Radial():
return radial();case CalibModelDto_Polynomial():
return polynomial(_that.order);}
}
/// A variant of `when` that fallback to returning `null`
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  radial,TResult? Function( int order)?  polynomial,}) {final _that = this;
switch (_that) {
case CalibModelDto_Radial() when radial != null:
return radial();case CalibModelDto_Polynomial() when polynomial != null:
return polynomial(_that.order);case _:
  return null;

}
}

}

/// @nodoc


class CalibModelDto_Radial extends CalibModelDto {
  const CalibModelDto_Radial(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is CalibModelDto_Radial);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'CalibModelDto.radial()';
}


}




/// @nodoc


class CalibModelDto_Polynomial extends CalibModelDto {
  const CalibModelDto_Polynomial({required this.order}): super._();
  

 final  int order;

/// Create a copy of CalibModelDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$CalibModelDto_PolynomialCopyWith<CalibModelDto_Polynomial> get copyWith => _$CalibModelDto_PolynomialCopyWithImpl<CalibModelDto_Polynomial>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is CalibModelDto_Polynomial&&(identical(other.order, order) || other.order == order));
}


@override
int get hashCode => Object.hash(runtimeType,order);

@override
String toString() {
  return 'CalibModelDto.polynomial(order: $order)';
}


}

/// @nodoc
abstract mixin class $CalibModelDto_PolynomialCopyWith<$Res> implements $CalibModelDtoCopyWith<$Res> {
  factory $CalibModelDto_PolynomialCopyWith(CalibModelDto_Polynomial value, $Res Function(CalibModelDto_Polynomial) _then) = _$CalibModelDto_PolynomialCopyWithImpl;
@useResult
$Res call({
 int order
});




}
/// @nodoc
class _$CalibModelDto_PolynomialCopyWithImpl<$Res>
    implements $CalibModelDto_PolynomialCopyWith<$Res> {
  _$CalibModelDto_PolynomialCopyWithImpl(this._self, this._then);

  final CalibModelDto_Polynomial _self;
  final $Res Function(CalibModelDto_Polynomial) _then;

/// Create a copy of CalibModelDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? order = null,}) {
  return _then(CalibModelDto_Polynomial(
order: null == order ? _self.order : order // ignore: cast_nullable_to_non_nullable
as int,
  ));
}


}

/// @nodoc
mixin _$DistortionDto {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DistortionDto);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'DistortionDto()';
}


}

/// @nodoc
class $DistortionDtoCopyWith<$Res>  {
$DistortionDtoCopyWith(DistortionDto _, $Res Function(DistortionDto) __);
}


/// Adds pattern-matching-related methods to [DistortionDto].
extension DistortionDtoPatterns on DistortionDto {
/// A variant of `map` that fallback to returning `orElse`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( DistortionDto_None value)?  none,TResult Function( DistortionDto_Radial value)?  radial,TResult Function( DistortionDto_Polynomial value)?  polynomial,required TResult orElse(),}){
final _that = this;
switch (_that) {
case DistortionDto_None() when none != null:
return none(_that);case DistortionDto_Radial() when radial != null:
return radial(_that);case DistortionDto_Polynomial() when polynomial != null:
return polynomial(_that);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// Callbacks receives the raw object, upcasted.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case final Subclass2 value:
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( DistortionDto_None value)  none,required TResult Function( DistortionDto_Radial value)  radial,required TResult Function( DistortionDto_Polynomial value)  polynomial,}){
final _that = this;
switch (_that) {
case DistortionDto_None():
return none(_that);case DistortionDto_Radial():
return radial(_that);case DistortionDto_Polynomial():
return polynomial(_that);}
}
/// A variant of `map` that fallback to returning `null`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( DistortionDto_None value)?  none,TResult? Function( DistortionDto_Radial value)?  radial,TResult? Function( DistortionDto_Polynomial value)?  polynomial,}){
final _that = this;
switch (_that) {
case DistortionDto_None() when none != null:
return none(_that);case DistortionDto_Radial() when radial != null:
return radial(_that);case DistortionDto_Polynomial() when polynomial != null:
return polynomial(_that);case _:
  return null;

}
}
/// A variant of `when` that fallback to an `orElse` callback.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  none,TResult Function( double k1,  double k2,  double k3,  double p1,  double p2,  double? centerX,  double? centerY)?  radial,TResult Function( int order,  double scale,  Float64List aCoeffs,  Float64List bCoeffs)?  polynomial,required TResult orElse(),}) {final _that = this;
switch (_that) {
case DistortionDto_None() when none != null:
return none();case DistortionDto_Radial() when radial != null:
return radial(_that.k1,_that.k2,_that.k3,_that.p1,_that.p2,_that.centerX,_that.centerY);case DistortionDto_Polynomial() when polynomial != null:
return polynomial(_that.order,_that.scale,_that.aCoeffs,_that.bCoeffs);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// As opposed to `map`, this offers destructuring.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case Subclass2(:final field2):
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  none,required TResult Function( double k1,  double k2,  double k3,  double p1,  double p2,  double? centerX,  double? centerY)  radial,required TResult Function( int order,  double scale,  Float64List aCoeffs,  Float64List bCoeffs)  polynomial,}) {final _that = this;
switch (_that) {
case DistortionDto_None():
return none();case DistortionDto_Radial():
return radial(_that.k1,_that.k2,_that.k3,_that.p1,_that.p2,_that.centerX,_that.centerY);case DistortionDto_Polynomial():
return polynomial(_that.order,_that.scale,_that.aCoeffs,_that.bCoeffs);}
}
/// A variant of `when` that fallback to returning `null`
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  none,TResult? Function( double k1,  double k2,  double k3,  double p1,  double p2,  double? centerX,  double? centerY)?  radial,TResult? Function( int order,  double scale,  Float64List aCoeffs,  Float64List bCoeffs)?  polynomial,}) {final _that = this;
switch (_that) {
case DistortionDto_None() when none != null:
return none();case DistortionDto_Radial() when radial != null:
return radial(_that.k1,_that.k2,_that.k3,_that.p1,_that.p2,_that.centerX,_that.centerY);case DistortionDto_Polynomial() when polynomial != null:
return polynomial(_that.order,_that.scale,_that.aCoeffs,_that.bCoeffs);case _:
  return null;

}
}

}

/// @nodoc


class DistortionDto_None extends DistortionDto {
  const DistortionDto_None(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DistortionDto_None);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'DistortionDto.none()';
}


}




/// @nodoc


class DistortionDto_Radial extends DistortionDto {
  const DistortionDto_Radial({required this.k1, required this.k2, required this.k3, required this.p1, required this.p2, this.centerX, this.centerY}): super._();
  

 final  double k1;
 final  double k2;
 final  double k3;
 final  double p1;
 final  double p2;
 final  double? centerX;
 final  double? centerY;

/// Create a copy of DistortionDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DistortionDto_RadialCopyWith<DistortionDto_Radial> get copyWith => _$DistortionDto_RadialCopyWithImpl<DistortionDto_Radial>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DistortionDto_Radial&&(identical(other.k1, k1) || other.k1 == k1)&&(identical(other.k2, k2) || other.k2 == k2)&&(identical(other.k3, k3) || other.k3 == k3)&&(identical(other.p1, p1) || other.p1 == p1)&&(identical(other.p2, p2) || other.p2 == p2)&&(identical(other.centerX, centerX) || other.centerX == centerX)&&(identical(other.centerY, centerY) || other.centerY == centerY));
}


@override
int get hashCode => Object.hash(runtimeType,k1,k2,k3,p1,p2,centerX,centerY);

@override
String toString() {
  return 'DistortionDto.radial(k1: $k1, k2: $k2, k3: $k3, p1: $p1, p2: $p2, centerX: $centerX, centerY: $centerY)';
}


}

/// @nodoc
abstract mixin class $DistortionDto_RadialCopyWith<$Res> implements $DistortionDtoCopyWith<$Res> {
  factory $DistortionDto_RadialCopyWith(DistortionDto_Radial value, $Res Function(DistortionDto_Radial) _then) = _$DistortionDto_RadialCopyWithImpl;
@useResult
$Res call({
 double k1, double k2, double k3, double p1, double p2, double? centerX, double? centerY
});




}
/// @nodoc
class _$DistortionDto_RadialCopyWithImpl<$Res>
    implements $DistortionDto_RadialCopyWith<$Res> {
  _$DistortionDto_RadialCopyWithImpl(this._self, this._then);

  final DistortionDto_Radial _self;
  final $Res Function(DistortionDto_Radial) _then;

/// Create a copy of DistortionDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? k1 = null,Object? k2 = null,Object? k3 = null,Object? p1 = null,Object? p2 = null,Object? centerX = freezed,Object? centerY = freezed,}) {
  return _then(DistortionDto_Radial(
k1: null == k1 ? _self.k1 : k1 // ignore: cast_nullable_to_non_nullable
as double,k2: null == k2 ? _self.k2 : k2 // ignore: cast_nullable_to_non_nullable
as double,k3: null == k3 ? _self.k3 : k3 // ignore: cast_nullable_to_non_nullable
as double,p1: null == p1 ? _self.p1 : p1 // ignore: cast_nullable_to_non_nullable
as double,p2: null == p2 ? _self.p2 : p2 // ignore: cast_nullable_to_non_nullable
as double,centerX: freezed == centerX ? _self.centerX : centerX // ignore: cast_nullable_to_non_nullable
as double?,centerY: freezed == centerY ? _self.centerY : centerY // ignore: cast_nullable_to_non_nullable
as double?,
  ));
}


}

/// @nodoc


class DistortionDto_Polynomial extends DistortionDto {
  const DistortionDto_Polynomial({required this.order, required this.scale, required this.aCoeffs, required this.bCoeffs}): super._();
  

 final  int order;
 final  double scale;
 final  Float64List aCoeffs;
 final  Float64List bCoeffs;

/// Create a copy of DistortionDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DistortionDto_PolynomialCopyWith<DistortionDto_Polynomial> get copyWith => _$DistortionDto_PolynomialCopyWithImpl<DistortionDto_Polynomial>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DistortionDto_Polynomial&&(identical(other.order, order) || other.order == order)&&(identical(other.scale, scale) || other.scale == scale)&&const DeepCollectionEquality().equals(other.aCoeffs, aCoeffs)&&const DeepCollectionEquality().equals(other.bCoeffs, bCoeffs));
}


@override
int get hashCode => Object.hash(runtimeType,order,scale,const DeepCollectionEquality().hash(aCoeffs),const DeepCollectionEquality().hash(bCoeffs));

@override
String toString() {
  return 'DistortionDto.polynomial(order: $order, scale: $scale, aCoeffs: $aCoeffs, bCoeffs: $bCoeffs)';
}


}

/// @nodoc
abstract mixin class $DistortionDto_PolynomialCopyWith<$Res> implements $DistortionDtoCopyWith<$Res> {
  factory $DistortionDto_PolynomialCopyWith(DistortionDto_Polynomial value, $Res Function(DistortionDto_Polynomial) _then) = _$DistortionDto_PolynomialCopyWithImpl;
@useResult
$Res call({
 int order, double scale, Float64List aCoeffs, Float64List bCoeffs
});




}
/// @nodoc
class _$DistortionDto_PolynomialCopyWithImpl<$Res>
    implements $DistortionDto_PolynomialCopyWith<$Res> {
  _$DistortionDto_PolynomialCopyWithImpl(this._self, this._then);

  final DistortionDto_Polynomial _self;
  final $Res Function(DistortionDto_Polynomial) _then;

/// Create a copy of DistortionDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? order = null,Object? scale = null,Object? aCoeffs = null,Object? bCoeffs = null,}) {
  return _then(DistortionDto_Polynomial(
order: null == order ? _self.order : order // ignore: cast_nullable_to_non_nullable
as int,scale: null == scale ? _self.scale : scale // ignore: cast_nullable_to_non_nullable
as double,aCoeffs: null == aCoeffs ? _self.aCoeffs : aCoeffs // ignore: cast_nullable_to_non_nullable
as Float64List,bCoeffs: null == bCoeffs ? _self.bCoeffs : bCoeffs // ignore: cast_nullable_to_non_nullable
as Float64List,
  ));
}


}

/// @nodoc
mixin _$ExtractionProfileDto {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is ExtractionProfileDto);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'ExtractionProfileDto()';
}


}

/// @nodoc
class $ExtractionProfileDtoCopyWith<$Res>  {
$ExtractionProfileDtoCopyWith(ExtractionProfileDto _, $Res Function(ExtractionProfileDto) __);
}


/// Adds pattern-matching-related methods to [ExtractionProfileDto].
extension ExtractionProfileDtoPatterns on ExtractionProfileDto {
/// A variant of `map` that fallback to returning `orElse`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( ExtractionProfileDto_Auto value)?  auto,TResult Function( ExtractionProfileDto_PhoneJpeg value)?  phoneJpeg,TResult Function( ExtractionProfileDto_CleanSensor value)?  cleanSensor,TResult Function( ExtractionProfileDto_CustomCcl value)?  customCcl,TResult Function( ExtractionProfileDto_CustomFast value)?  customFast,required TResult orElse(),}){
final _that = this;
switch (_that) {
case ExtractionProfileDto_Auto() when auto != null:
return auto(_that);case ExtractionProfileDto_PhoneJpeg() when phoneJpeg != null:
return phoneJpeg(_that);case ExtractionProfileDto_CleanSensor() when cleanSensor != null:
return cleanSensor(_that);case ExtractionProfileDto_CustomCcl() when customCcl != null:
return customCcl(_that);case ExtractionProfileDto_CustomFast() when customFast != null:
return customFast(_that);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// Callbacks receives the raw object, upcasted.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case final Subclass2 value:
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( ExtractionProfileDto_Auto value)  auto,required TResult Function( ExtractionProfileDto_PhoneJpeg value)  phoneJpeg,required TResult Function( ExtractionProfileDto_CleanSensor value)  cleanSensor,required TResult Function( ExtractionProfileDto_CustomCcl value)  customCcl,required TResult Function( ExtractionProfileDto_CustomFast value)  customFast,}){
final _that = this;
switch (_that) {
case ExtractionProfileDto_Auto():
return auto(_that);case ExtractionProfileDto_PhoneJpeg():
return phoneJpeg(_that);case ExtractionProfileDto_CleanSensor():
return cleanSensor(_that);case ExtractionProfileDto_CustomCcl():
return customCcl(_that);case ExtractionProfileDto_CustomFast():
return customFast(_that);}
}
/// A variant of `map` that fallback to returning `null`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( ExtractionProfileDto_Auto value)?  auto,TResult? Function( ExtractionProfileDto_PhoneJpeg value)?  phoneJpeg,TResult? Function( ExtractionProfileDto_CleanSensor value)?  cleanSensor,TResult? Function( ExtractionProfileDto_CustomCcl value)?  customCcl,TResult? Function( ExtractionProfileDto_CustomFast value)?  customFast,}){
final _that = this;
switch (_that) {
case ExtractionProfileDto_Auto() when auto != null:
return auto(_that);case ExtractionProfileDto_PhoneJpeg() when phoneJpeg != null:
return phoneJpeg(_that);case ExtractionProfileDto_CleanSensor() when cleanSensor != null:
return cleanSensor(_that);case ExtractionProfileDto_CustomCcl() when customCcl != null:
return customCcl(_that);case ExtractionProfileDto_CustomFast() when customFast != null:
return customFast(_that);case _:
  return null;

}
}
/// A variant of `when` that fallback to an `orElse` callback.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  auto,TResult Function()?  phoneJpeg,TResult Function()?  cleanSensor,TResult Function( double sigma,  int maxCentroids)?  customCcl,TResult Function( double sigma,  int maxCentroids)?  customFast,required TResult orElse(),}) {final _that = this;
switch (_that) {
case ExtractionProfileDto_Auto() when auto != null:
return auto();case ExtractionProfileDto_PhoneJpeg() when phoneJpeg != null:
return phoneJpeg();case ExtractionProfileDto_CleanSensor() when cleanSensor != null:
return cleanSensor();case ExtractionProfileDto_CustomCcl() when customCcl != null:
return customCcl(_that.sigma,_that.maxCentroids);case ExtractionProfileDto_CustomFast() when customFast != null:
return customFast(_that.sigma,_that.maxCentroids);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// As opposed to `map`, this offers destructuring.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case Subclass2(:final field2):
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  auto,required TResult Function()  phoneJpeg,required TResult Function()  cleanSensor,required TResult Function( double sigma,  int maxCentroids)  customCcl,required TResult Function( double sigma,  int maxCentroids)  customFast,}) {final _that = this;
switch (_that) {
case ExtractionProfileDto_Auto():
return auto();case ExtractionProfileDto_PhoneJpeg():
return phoneJpeg();case ExtractionProfileDto_CleanSensor():
return cleanSensor();case ExtractionProfileDto_CustomCcl():
return customCcl(_that.sigma,_that.maxCentroids);case ExtractionProfileDto_CustomFast():
return customFast(_that.sigma,_that.maxCentroids);}
}
/// A variant of `when` that fallback to returning `null`
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  auto,TResult? Function()?  phoneJpeg,TResult? Function()?  cleanSensor,TResult? Function( double sigma,  int maxCentroids)?  customCcl,TResult? Function( double sigma,  int maxCentroids)?  customFast,}) {final _that = this;
switch (_that) {
case ExtractionProfileDto_Auto() when auto != null:
return auto();case ExtractionProfileDto_PhoneJpeg() when phoneJpeg != null:
return phoneJpeg();case ExtractionProfileDto_CleanSensor() when cleanSensor != null:
return cleanSensor();case ExtractionProfileDto_CustomCcl() when customCcl != null:
return customCcl(_that.sigma,_that.maxCentroids);case ExtractionProfileDto_CustomFast() when customFast != null:
return customFast(_that.sigma,_that.maxCentroids);case _:
  return null;

}
}

}

/// @nodoc


class ExtractionProfileDto_Auto extends ExtractionProfileDto {
  const ExtractionProfileDto_Auto(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is ExtractionProfileDto_Auto);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'ExtractionProfileDto.auto()';
}


}




/// @nodoc


class ExtractionProfileDto_PhoneJpeg extends ExtractionProfileDto {
  const ExtractionProfileDto_PhoneJpeg(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is ExtractionProfileDto_PhoneJpeg);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'ExtractionProfileDto.phoneJpeg()';
}


}




/// @nodoc


class ExtractionProfileDto_CleanSensor extends ExtractionProfileDto {
  const ExtractionProfileDto_CleanSensor(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is ExtractionProfileDto_CleanSensor);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'ExtractionProfileDto.cleanSensor()';
}


}




/// @nodoc


class ExtractionProfileDto_CustomCcl extends ExtractionProfileDto {
  const ExtractionProfileDto_CustomCcl({required this.sigma, required this.maxCentroids}): super._();
  

 final  double sigma;
 final  int maxCentroids;

/// Create a copy of ExtractionProfileDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$ExtractionProfileDto_CustomCclCopyWith<ExtractionProfileDto_CustomCcl> get copyWith => _$ExtractionProfileDto_CustomCclCopyWithImpl<ExtractionProfileDto_CustomCcl>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is ExtractionProfileDto_CustomCcl&&(identical(other.sigma, sigma) || other.sigma == sigma)&&(identical(other.maxCentroids, maxCentroids) || other.maxCentroids == maxCentroids));
}


@override
int get hashCode => Object.hash(runtimeType,sigma,maxCentroids);

@override
String toString() {
  return 'ExtractionProfileDto.customCcl(sigma: $sigma, maxCentroids: $maxCentroids)';
}


}

/// @nodoc
abstract mixin class $ExtractionProfileDto_CustomCclCopyWith<$Res> implements $ExtractionProfileDtoCopyWith<$Res> {
  factory $ExtractionProfileDto_CustomCclCopyWith(ExtractionProfileDto_CustomCcl value, $Res Function(ExtractionProfileDto_CustomCcl) _then) = _$ExtractionProfileDto_CustomCclCopyWithImpl;
@useResult
$Res call({
 double sigma, int maxCentroids
});




}
/// @nodoc
class _$ExtractionProfileDto_CustomCclCopyWithImpl<$Res>
    implements $ExtractionProfileDto_CustomCclCopyWith<$Res> {
  _$ExtractionProfileDto_CustomCclCopyWithImpl(this._self, this._then);

  final ExtractionProfileDto_CustomCcl _self;
  final $Res Function(ExtractionProfileDto_CustomCcl) _then;

/// Create a copy of ExtractionProfileDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? sigma = null,Object? maxCentroids = null,}) {
  return _then(ExtractionProfileDto_CustomCcl(
sigma: null == sigma ? _self.sigma : sigma // ignore: cast_nullable_to_non_nullable
as double,maxCentroids: null == maxCentroids ? _self.maxCentroids : maxCentroids // ignore: cast_nullable_to_non_nullable
as int,
  ));
}


}

/// @nodoc


class ExtractionProfileDto_CustomFast extends ExtractionProfileDto {
  const ExtractionProfileDto_CustomFast({required this.sigma, required this.maxCentroids}): super._();
  

 final  double sigma;
 final  int maxCentroids;

/// Create a copy of ExtractionProfileDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$ExtractionProfileDto_CustomFastCopyWith<ExtractionProfileDto_CustomFast> get copyWith => _$ExtractionProfileDto_CustomFastCopyWithImpl<ExtractionProfileDto_CustomFast>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is ExtractionProfileDto_CustomFast&&(identical(other.sigma, sigma) || other.sigma == sigma)&&(identical(other.maxCentroids, maxCentroids) || other.maxCentroids == maxCentroids));
}


@override
int get hashCode => Object.hash(runtimeType,sigma,maxCentroids);

@override
String toString() {
  return 'ExtractionProfileDto.customFast(sigma: $sigma, maxCentroids: $maxCentroids)';
}


}

/// @nodoc
abstract mixin class $ExtractionProfileDto_CustomFastCopyWith<$Res> implements $ExtractionProfileDtoCopyWith<$Res> {
  factory $ExtractionProfileDto_CustomFastCopyWith(ExtractionProfileDto_CustomFast value, $Res Function(ExtractionProfileDto_CustomFast) _then) = _$ExtractionProfileDto_CustomFastCopyWithImpl;
@useResult
$Res call({
 double sigma, int maxCentroids
});




}
/// @nodoc
class _$ExtractionProfileDto_CustomFastCopyWithImpl<$Res>
    implements $ExtractionProfileDto_CustomFastCopyWith<$Res> {
  _$ExtractionProfileDto_CustomFastCopyWithImpl(this._self, this._then);

  final ExtractionProfileDto_CustomFast _self;
  final $Res Function(ExtractionProfileDto_CustomFast) _then;

/// Create a copy of ExtractionProfileDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? sigma = null,Object? maxCentroids = null,}) {
  return _then(ExtractionProfileDto_CustomFast(
sigma: null == sigma ? _self.sigma : sigma // ignore: cast_nullable_to_non_nullable
as double,maxCentroids: null == maxCentroids ? _self.maxCentroids : maxCentroids // ignore: cast_nullable_to_non_nullable
as int,
  ));
}


}

// dart format on
