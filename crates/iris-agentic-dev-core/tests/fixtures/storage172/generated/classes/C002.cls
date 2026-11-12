Class IadSm172Gen.C002 Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property K1 As %Integer;

Property F1 As %String(COLLATION = "EXACT");

Property F2 As %String;

Property F3 As %Integer;

Index IDKEY On K1 [ IdKey, Unique ];

Index IdxF1 On F1;

Storage SQLStorage
{
<SQLMap name="IdxF1">
<Global>^IadSm172Gen002I</Global>
<Subscript name="1">
<Expression>{F1}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{K1}</Expression>
</Subscript>
<Type>index</Type>
</SQLMap>
<SQLMap name="Master">
<Data name="F1">
<Delimiter>"|"</Delimiter>
<Node>"d"</Node>
<Piece>1</Piece>
</Data>
<Data name="F2">
<Delimiter>"|"</Delimiter>
<Node>"d"</Node>
<Piece>2</Piece>
</Data>
<Data name="F3">
<RetrievalCode> s {*}={L1}</RetrievalCode>
</Data>
<Global>^IadSm172Gen002</Global>
<Subscript name="1">
<AccessType>Sub</AccessType>
<Expression>{K1}</Expression>
<NextCode> s {L1}=$o(^IadSm172Gen002({L1}))
 f  {
	q:{L1}=""
	q:{L1}=+{L1}
	s {L1}=$o(^IadSm172Gen002({L1}))
 }</NextCode>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172Gen002S</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
