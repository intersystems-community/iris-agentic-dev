Class IadSm172.Shelf Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property ShelfArea As %String;

Property ShelfLoc As %String;

Storage SQLStorage
{
<SqlIdExpression>$i(^IadSm172Shelf)</SqlIdExpression>
<SQLMap name="Master">
<Data name="ShelfArea">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Data name="ShelfLoc">
<Delimiter>"^"</Delimiter>
<Piece>2</Piece>
</Data>
<Global>^IadSm172Shelf</Global>
<Subscript name="1">
<Expression>{ID}</Expression>
<NextCode> s {L1}=$o(^IadSm172Shelf({L1}))
 f  {
	q:{L1}=""
	q:{L1}'="reserved"
	s {L1}=$o(^IadSm172Shelf({L1}))
 }</NextCode>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172ShelfS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}

