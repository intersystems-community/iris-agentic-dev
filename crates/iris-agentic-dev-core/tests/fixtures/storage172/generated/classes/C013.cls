Class IadSm172Gen.C013 Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property K1 As %Integer;

Property K2 As %Integer;

Property K3 As %Integer;

Property F1 As %String(COLLATION = "EXACT");

Property F2 As %String;

Index IDKEY On (K1, K2, K3) [ IdKey, Unique ];

Storage SQLStorage
{
<SQLMap name="Master">
<Data name="F1">
<Delimiter>"|"</Delimiter>
<Node>"a"</Node>
<Piece>1</Piece>
</Data>
<Data name="F2">
<Delimiter>"|"</Delimiter>
<Node>"b"</Node>
<Piece>1</Piece>
</Data>
<Global>^IadSm172Gen013</Global>
<Subscript name="1">
<Expression>{K1}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{K2}</Expression>
</Subscript>
<Subscript name="3">
<Expression>{K3}</Expression>
<NextCode> s {L3}=$o(^IadSm172Gen013({L1},{L2},{L3}))
 f  {
	q:{L3}=""
	q:{L3}=+{L3}
	s {L3}=$o(^IadSm172Gen013({L1},{L2},{L3}))
 }</NextCode>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172Gen013S</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
