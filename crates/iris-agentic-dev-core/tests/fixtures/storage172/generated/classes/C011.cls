Class IadSm172Gen.C011 Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property K1 As %Integer;

Property K2 As %Integer;

Property K3 As %Integer;

Property F1 As %String(COLLATION = "EXACT");

Property F2 As %String;

Property F3 As %Integer;

Index IDKEY On (K1, K2, K3) [ IdKey, Unique ];

Index IdxF1 On F1;

Storage SQLStorage
{
<SQLMap name="IdxF1">
<Global>^IadSm172Gen011I</Global>
<Subscript name="1">
<Expression>{F1}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{K1}</Expression>
</Subscript>
<Subscript name="3">
<Expression>{K2}</Expression>
</Subscript>
<Subscript name="4">
<Expression>{K3}</Expression>
</Subscript>
<Type>index</Type>
</SQLMap>
<SQLMap name="Master">
<Data name="F1">
<Delimiter>$C(1)</Delimiter>
<Piece>1</Piece>
</Data>
<Data name="F2">
<Delimiter>$C(1)</Delimiter>
<Node>"d"</Node>
<Piece>1</Piece>
</Data>
<Data name="F3">
<RetrievalCode> s {*}={L1}</RetrievalCode>
</Data>
<Global>^IadSm172Gen011</Global>
<Subscript name="1">
<Expression>{K1}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{K2}</Expression>
</Subscript>
<Subscript name="3">
<Expression>{K3}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172Gen011S</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
