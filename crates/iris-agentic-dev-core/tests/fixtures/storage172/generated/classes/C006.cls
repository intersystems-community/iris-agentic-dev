Class IadSm172Gen.C006 Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property K1 As %Integer;

Property K2 As %Integer;

Property F1 As %String(COLLATION = "EXACT");

Property F2 As %String;

Property F3 As %Integer;

Index IDKEY On (K1, K2) [ IdKey, Unique ];

Storage SQLStorage
{
<SQLMap name="Master">
<Data name="F1">
<Delimiter>"|"</Delimiter>
<Piece>1</Piece>
</Data>
<Data name="F2">
<Delimiter>"|"</Delimiter>
<Piece>2</Piece>
</Data>
<Data name="F3">
<RetrievalCode> s {*}={L1}</RetrievalCode>
</Data>
<Global>^IadSm172Gen006</Global>
<Subscript name="1">
<Expression>{K1}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{K2}</Expression>
<NextCode> s {L2}=$o(^IadSm172Gen006({L1},{L2}))
 f  {
	q:{L2}=""
	q:{L2}=+{L2}
	s {L2}=$o(^IadSm172Gen006({L1},{L2}))
 }</NextCode>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172Gen006S</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
