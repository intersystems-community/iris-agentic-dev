Class IadSm172Gen.C008 Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property K1 As %Integer;

Property K2 As %Integer;

Property F1 As %String(COLLATION = "EXACT");

Property F2 As %String;

Index IDKEY On (K1, K2) [ IdKey, Unique ];

Index IdxF1 On F1;

Storage SQLStorage
{
<SQLMap name="IdxF1">
<Global>^IadSm172Gen008I</Global>
<Subscript name="1">
<Expression>{F1}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{K1}</Expression>
</Subscript>
<Subscript name="3">
<Expression>{K2}</Expression>
</Subscript>
<Type>index</Type>
</SQLMap>
<SQLMap name="Master">
<Data name="F1">
<Piece>1</Piece>
</Data>
<Data name="F2">
<Node>"d"</Node>
<Piece>1</Piece>
</Data>
<Global>^IadSm172Gen008</Global>
<Subscript name="1">
<Expression>{K1}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{K2}</Expression>
<NextCode> s {L2}=$o(^IadSm172Gen008({L1},{L2}))
 f  {
	q:{L2}=""
	q:{L2}=+{L2}
	s {L2}=$o(^IadSm172Gen008({L1},{L2}))
 }</NextCode>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172Gen008S</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
